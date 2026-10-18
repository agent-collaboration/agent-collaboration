use crate::*;
use collaboration_replication_protocol::{
    Piece, ProjectId, ReplicationPosition, SchemaIdentity, SessionAnswer, SessionRefusal,
};

use crate::copy_receipts::{drop_parts, receipt_digest, save_part, save_receipt, staged_parts};
use crate::copy_storage::{load_copy, save_copy};
use crate::row_apply::{apply_transactions, schema_fingerprint};
use crate::{CentralError, CopyStatus, RegistryTime, SqlTx, StoreError};

pub(crate) fn initialize(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
    epoch: collaboration_replication_protocol::Epoch,
    holder: &collaboration_replication_protocol::InstallationId,
) -> Result<CopyStatus, CentralError> {
    if let Some(state) = load_copy(tx)? {
        if &state.project != project {
            return Err(CentralError::InvalidInput {
                message: "copy already belongs to a different project".into(),
            });
        }
        return Ok(state);
    }
    let position = ReplicationPosition::new(epoch, 0.into());
    let state = CopyStatus {
        project: project.clone(),
        fence: epoch,
        holder: Some(holder.clone()),
        imported: None,
        start: position,
        acked: position,
        schema: SchemaIdentity::new(0, schema_fingerprint(tx)?).map_err(|_| {
            StoreError::Corrupt {
                field: "schema_fingerprint".into(),
            }
        })?,
        archived: false,
    };
    save_copy(tx, &state)?;
    Ok(state)
}

fn refused(reason: SessionRefusal) -> SessionAnswer {
    SessionAnswer::Refused(reason)
}

pub(crate) fn apply_piece(
    tx: &mut dyn SqlTx,
    piece: &Piece,
    sender: &collaboration_replication_protocol::InstallationId,
    now: RegistryTime,
    ids: &mut crate::IdBatch,
) -> Result<(SessionAnswer, Vec<CopyEffect>), CentralError> {
    let Some(mut state) = load_copy(tx)? else {
        return Ok((refused(SessionRefusal::NotInitialized), Vec::new()));
    };
    if state.project != piece.project {
        return Ok((refused(SessionRefusal::NotInitialized), Vec::new()));
    }
    if state.holder.is_none() {
        return Ok((refused(SessionRefusal::NotHeld), Vec::new()));
    }
    if piece.epoch < state.fence {
        if crate::fence_rules::epoch_holder(tx, piece.epoch)?.as_ref() != Some(sender) {
            return Ok((
                refused(SessionRefusal::NotWriter {
                    current: Some(state.fence),
                }),
                Vec::new(),
            ));
        }
        let cutoff = crate::fence_rules::epoch_cutoff(tx, piece.epoch)?;
        return Ok((
            refused(SessionRefusal::StaleEpoch {
                current_epoch: state.fence,
                epoch_ended_at: cutoff,
            }),
            Vec::new(),
        ));
    }
    if piece.epoch != state.fence || state.holder.as_ref() != Some(sender) {
        return Ok((
            refused(SessionRefusal::NotWriter {
                current: Some(state.fence),
            }),
            Vec::new(),
        ));
    }
    if piece
        .recomputed_digest()
        .map_err(|error| CentralError::Encoding {
            message: error.to_string(),
        })?
        != piece.digest
    {
        return Ok((refused(SessionRefusal::PieceConflict), Vec::new()));
    }
    if let Some(digest) = receipt_digest(tx, piece)? {
        let answer = if digest == piece.digest {
            SessionAnswer::Acked {
                through: piece.through,
            }
        } else {
            refused(SessionRefusal::PieceConflict)
        };
        return Ok((answer, Vec::new()));
    }
    if piece.through <= state.acked || piece.continues_from != state.acked {
        return Ok((
            refused(SessionRefusal::PositionGap { acked: state.acked }),
            Vec::new(),
        ));
    }
    // Admit the whole incoming piece before staging or executing any of it.
    if let Some(form) = unsupported_schema_form(&piece.transactions) {
        return Ok((
            refused(SessionRefusal::SchemaUnsupported { form }),
            Vec::new(),
        ));
    }
    if let Some(part) = &piece.part {
        let mut staged = staged_parts(tx, part.transaction)?;
        if let Some(repeated) = staged.iter().find(|existing| {
            existing
                .part
                .as_ref()
                .is_some_and(|existing| existing.index == part.index)
        }) {
            let answer = if repeated.digest == piece.digest {
                SessionAnswer::PartStaged {
                    transaction: part.transaction,
                    index: part.index,
                    acked: state.acked,
                }
            } else {
                refused(SessionRefusal::PieceConflict)
            };
            return Ok((answer, Vec::new()));
        }
        if part.index < part.count - 1 {
            if staged.iter().any(|existing| {
                existing.continues_from != piece.continues_from
                    || existing.through != piece.through
                    || existing.part.as_ref().is_none_or(|existing| {
                        existing.count != part.count
                            || existing.transaction_digest != part.transaction_digest
                    })
            }) {
                return Ok((refused(SessionRefusal::PieceConflict), Vec::new()));
            }
            save_part(tx, piece, now)?;
            crate::copy_piece_time::record_new_piece_time(tx, now)?;
            let alarm =
                crate::copy_receipts::next_part_alarm(tx)?.ok_or_else(|| StoreError::Corrupt {
                    field: "staged_alarm".into(),
                })?;
            return Ok((
                SessionAnswer::PartStaged {
                    transaction: part.transaction,
                    index: part.index,
                    acked: state.acked,
                },
                vec![CopyEffect::SetAlarm { at: alarm }],
            ));
        }
        staged.push(piece.clone());
        let Some(transaction) = crate::part_assembly::assemble_transaction(&staged, piece) else {
            drop_parts(tx, part.transaction)?;
            return Ok((
                refused(SessionRefusal::PositionGap { acked: state.acked }),
                Vec::new(),
            ));
        };
        if let Some(form) = unsupported_schema_form(std::slice::from_ref(&transaction)) {
            return Ok((
                refused(SessionRefusal::SchemaUnsupported { form }),
                Vec::new(),
            ));
        }
        apply_transactions(tx, &mut state, &[transaction])?;
        for applied in &staged {
            save_receipt(tx, applied)?;
        }
        drop_parts(tx, part.transaction)?;
    } else {
        apply_transactions(tx, &mut state, &piece.transactions)?;
        save_receipt(tx, piece)?;
    }
    state.acked = piece.through;
    save_copy(tx, &state)?;
    let summary = piece
        .summary
        .as_ref()
        .ok_or_else(|| CentralError::InvalidInput {
            message: "complete transaction must carry its summary".into(),
        })?;
    let effects =
        crate::directory_producer::summary(tx, &state.project, summary, piece.through, now, ids)?;
    crate::copy_piece_time::record_new_piece_time(tx, now)?;
    Ok((
        SessionAnswer::Acked {
            through: piece.through,
        },
        effects,
    ))
}

fn unsupported_schema_form(
    transactions: &[collaboration_replication_protocol::ReplicatedTransaction],
) -> Option<collaboration_replication_protocol::SchemaStatementForm> {
    transactions.iter().find_map(|transaction| {
        let collaboration_replication_protocol::ReplicatedTransaction::SchemaStep {
            statements,
            ..
        } = transaction
        else {
            return None;
        };
        statements.iter().find_map(|sql| {
            collaboration_replication_protocol::admit_schema_statement(sql)
                .err()
                .map(|error| error.form)
        })
    })
}
