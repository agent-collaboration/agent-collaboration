use crate::copy_storage::query_statement;
use collaboration_replication_protocol::{Piece, PieceDigest, SqlValue, TransactionKey};

use crate::copy_storage::{decode_record, encode_record, statement, text, unsigned};
use crate::{CentralError, SqlTx, StoreError};

fn identity_parameters(piece: &Piece) -> Vec<SqlValue> {
    vec![
        unsigned(piece.epoch.value()),
        unsigned(piece.continues_from.epoch.value()),
        unsigned(piece.continues_from.commit_id.value()),
        unsigned(piece.through.epoch.value()),
        unsigned(piece.through.commit_id.value()),
    ]
}

pub(crate) fn receipt_digest(
    tx: &mut dyn SqlTx,
    piece: &Piece,
) -> Result<Option<PieceDigest>, CentralError> {
    let mut parameters = identity_parameters(piece);
    let sql = if let Some(part) = &piece.part {
        parameters.push(SqlValue::Integer(i64::from(part.index)));
        "SELECT digest FROM _copy_part_receipts WHERE epoch=? AND from_epoch=? AND from_commit=? AND through_epoch=? AND through_commit=? AND part_index=?"
    } else {
        "SELECT digest FROM _copy_receipts WHERE epoch=? AND from_epoch=? AND from_commit=? AND through_epoch=? AND through_commit=?"
    };
    let rows = tx.query(query_statement(sql, parameters, ["digest"]))?;
    rows.first()
        .map(|row| {
            PieceDigest::try_from(row.text("digest")?.to_owned()).map_err(|_| {
                StoreError::Corrupt {
                    field: "digest".into(),
                }
                .into()
            })
        })
        .transpose()
}

pub(crate) fn save_receipt(tx: &mut dyn SqlTx, piece: &Piece) -> Result<(), CentralError> {
    let mut parameters = identity_parameters(piece);
    let sql = if let Some(part) = &piece.part {
        parameters.push(SqlValue::Integer(i64::from(part.index)));
        "INSERT INTO _copy_part_receipts(epoch, from_epoch, from_commit, through_epoch, through_commit, part_index, digest) VALUES(?, ?, ?, ?, ?, ?, ?)"
    } else {
        "INSERT INTO _copy_receipts(epoch, from_epoch, from_commit, through_epoch, through_commit, digest) VALUES(?, ?, ?, ?, ?, ?)"
    };
    parameters.push(text(piece.digest.as_str()));
    tx.execute(statement(sql, parameters))?;
    Ok(())
}

pub(crate) fn staged_parts(
    tx: &mut dyn SqlTx,
    key: TransactionKey,
) -> Result<Vec<Piece>, CentralError> {
    let rows = tx.query(query_statement("SELECT part_index, payload FROM _copy_staged_parts WHERE transaction_epoch=? AND transaction_commit=? ORDER BY part_index", vec![unsigned(key.epoch.value()), unsigned(key.commit_id.value())], ["part_index", "payload"]))?;
    rows.iter()
        .map(|row| {
            let piece: Piece = decode_record(row.text("payload")?, "staged_part")?;
            let part = piece.part.as_ref().ok_or_else(|| StoreError::Corrupt {
                field: "staged_part".into(),
            })?;
            if part.transaction != key
                || i64::from(part.index) != row.integer("part_index")?
                || piece.recomputed_digest().map_err(|_| StoreError::Corrupt {
                    field: "staged_digest".into(),
                })? != piece.digest
            {
                return Err(StoreError::Corrupt {
                    field: "staged_part".into(),
                }
                .into());
            }
            Ok(piece)
        })
        .collect()
}

pub(crate) fn save_part(
    tx: &mut dyn SqlTx,
    piece: &Piece,
    now: crate::RegistryTime,
) -> Result<(), CentralError> {
    let part = piece
        .part
        .as_ref()
        .ok_or_else(|| CentralError::InvalidInput {
            message: "staged piece must be a part".into(),
        })?;
    tx.execute(statement("INSERT INTO _copy_staged_parts(transaction_epoch, transaction_commit, part_index, staged_at, payload) VALUES(?, ?, ?, ?, ?)", vec![unsigned(part.transaction.epoch.value()), unsigned(part.transaction.commit_id.value()), SqlValue::Integer(i64::from(part.index)), text(now.0.to_rfc3339()), text(encode_record(piece)?)]))?;
    Ok(())
}

pub(crate) fn drop_parts(tx: &mut dyn SqlTx, key: TransactionKey) -> Result<(), CentralError> {
    tx.execute(statement(
        "DELETE FROM _copy_staged_parts WHERE transaction_epoch=? AND transaction_commit=?",
        vec![unsigned(key.epoch.value()), unsigned(key.commit_id.value())],
    ))?;
    Ok(())
}

pub(crate) fn cleanup_parts(
    tx: &mut dyn SqlTx,
    now: crate::RegistryTime,
) -> Result<(), CentralError> {
    let Some(state) = crate::copy_storage::load_copy(tx)? else {
        return Ok(());
    };
    let rows = tx.query(query_statement(
        "SELECT transaction_epoch, transaction_commit, staged_at FROM _copy_staged_parts",
        Vec::new(),
        ["transaction_epoch", "transaction_commit", "staged_at"],
    ))?;
    for row in rows {
        let epoch = crate::copy_storage::epoch_field(&row, "transaction_epoch")?;
        let created_at =
            chrono::DateTime::parse_from_rfc3339(row.text("staged_at")?).map_err(|_| {
                StoreError::Corrupt {
                    field: "staged_at".into(),
                }
            })?;
        if epoch < state.fence
            || now.0.signed_duration_since(created_at) > chrono::TimeDelta::days(1)
        {
            drop_parts(
                tx,
                TransactionKey {
                    epoch,
                    commit_id: row.unsigned("transaction_commit")?.into(),
                },
            )?;
        }
    }
    Ok(())
}

pub(crate) fn next_part_alarm(
    tx: &mut dyn SqlTx,
) -> Result<Option<crate::RegistryTime>, CentralError> {
    let rows = tx.query(query_statement(
        "SELECT staged_at FROM _copy_staged_parts",
        Vec::new(),
        ["staged_at"],
    ))?;
    let mut earliest = None;
    for row in rows {
        let staged_at =
            chrono::DateTime::parse_from_rfc3339(row.text("staged_at")?).map_err(|_| {
                StoreError::Corrupt {
                    field: "staged_at".into(),
                }
            })?;
        let deadline = staged_at
            .checked_add_signed(chrono::TimeDelta::days(1) + chrono::TimeDelta::milliseconds(1))
            .ok_or_else(|| StoreError::Corrupt {
                field: "staged_at".into(),
            })?
            .with_timezone(&chrono::Utc);
        if earliest.is_none_or(|existing| deadline < existing) {
            earliest = Some(deadline);
        }
    }
    Ok(earliest.map(crate::RegistryTime))
}
