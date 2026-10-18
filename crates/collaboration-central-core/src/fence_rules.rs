use crate::copy_storage::query_statement;
use collaboration_replication_protocol::{Epoch, ReplicationPosition, TransferId};

use crate::copy_storage::{
    epoch_field, load_copy, position_fields, save_copy, statement, text, unsigned,
};
use crate::{CentralError, FenceExpectation, FenceResult, SqlTx};

pub(crate) fn fence(
    tx: &mut dyn SqlTx,
    transfer_id: &TransferId,
    new_epoch: Epoch,
    expect: &FenceExpectation,
    target_schema_max: u64,
    holder: &collaboration_replication_protocol::InstallationId,
) -> Result<FenceResult, CentralError> {
    let rows = tx.query(query_statement("SELECT old_epoch, new_epoch, start_epoch, start_commit FROM _copy_transfers WHERE transfer_id=?", vec![text(transfer_id.as_str())], ["old_epoch", "new_epoch", "start_epoch", "start_commit"]))?;
    if let Some(row) = rows.first() {
        let state = load_copy(tx)?.ok_or_else(|| crate::StoreError::Corrupt {
            field: "fence receipt state".into(),
        })?;
        if epoch_field(row, "new_epoch")? != state.fence || state.holder.as_ref() != Some(holder) {
            return Err(crate::StoreError::Corrupt {
                field: "fence receipt state".into(),
            }
            .into());
        }
        return Ok(FenceResult::Fenced {
            old_epoch: epoch_field(row, "old_epoch")?,
            new_epoch: epoch_field(row, "new_epoch")?,
            start: position_fields(row, "start")?,
        });
    }
    let Some(mut state) = load_copy(tx)? else {
        return Ok(FenceResult::NotInitialized);
    };
    if state.imported.as_ref().is_some_and(|imported| {
        expect.epoch == imported.import_epoch && state.fence != imported.import_epoch
    }) {
        return Ok(FenceResult::PreviewStale { acked: state.acked });
    }
    if new_epoch <= state.fence {
        return Ok(FenceResult::FenceNotAdvanced {
            current_epoch: state.fence,
        });
    }
    if state.fence != expect.epoch || state.acked != expect.acked {
        return Ok(FenceResult::PreviewStale { acked: state.acked });
    }
    if state.schema.version > target_schema_max {
        return Ok(FenceResult::SchemaTooNew {
            store: state.schema,
            supported: target_schema_max,
        });
    }
    let old_epoch = state.fence;
    let old_holder = state.holder.clone();
    state.fence = new_epoch;
    state.holder = Some(holder.clone());
    state.start = state.acked;
    save_copy(tx, &state)?;
    tx.execute(statement("INSERT INTO _copy_transfers(transfer_id, old_epoch, new_epoch, start_epoch, start_commit, old_holder) VALUES(?, ?, ?, ?, ?, ?)", vec![text(transfer_id.as_str()), unsigned(old_epoch.value()), unsigned(new_epoch.value()), unsigned(state.start.epoch.value()), unsigned(state.start.commit_id.value()), old_holder.as_ref().map_or(collaboration_replication_protocol::SqlValue::Null,|holder|text(holder.as_str()))]))?;
    Ok(FenceResult::Fenced {
        old_epoch,
        new_epoch,
        start: state.start,
    })
}

pub(crate) fn epoch_cutoff(
    tx: &mut dyn SqlTx,
    epoch: Epoch,
) -> Result<ReplicationPosition, CentralError> {
    let rows = tx.query(query_statement(
        "SELECT start_epoch, start_commit FROM _copy_transfers WHERE old_epoch=?",
        vec![unsigned(epoch.value())],
        ["start_epoch", "start_commit"],
    ))?;
    let row = rows.first().ok_or_else(|| crate::StoreError::Corrupt {
        field: "epoch_cutoff".into(),
    })?;
    if rows.len() != 1 {
        return Err(crate::StoreError::Corrupt {
            field: "epoch_cutoff".into(),
        }
        .into());
    }
    Ok(position_fields(row, "start")?)
}

pub(crate) fn epoch_holder(
    tx: &mut dyn SqlTx,
    epoch: Epoch,
) -> Result<Option<collaboration_replication_protocol::InstallationId>, CentralError> {
    let rows = tx.query(query_statement(
        "SELECT old_holder FROM _copy_transfers WHERE old_epoch=?",
        vec![unsigned(epoch.value())],
        ["old_holder"],
    ))?;
    if rows.len() > 1 {
        return Err(crate::StoreError::Corrupt {
            field: "epoch holder cardinality".into(),
        }
        .into());
    }
    if rows.first().is_some_and(|r| {
        matches!(
            r.value("old_holder"),
            Ok(collaboration_replication_protocol::SqlValue::Null)
        )
    }) {
        return Ok(None);
    }
    rows.first()
        .map(|r| {
            collaboration_replication_protocol::InstallationId::new(r.text("old_holder")?).map_err(
                |_| {
                    crate::StoreError::Corrupt {
                        field: "epoch holder".into(),
                    }
                    .into()
                },
            )
        })
        .transpose()
}
