//! Retired state is read before bookkeeping admission, including after the wipe.
use chrono::{DateTime, Utc};
use collaboration_replication_protocol::{Epoch, ReplicationPosition, TransferId};
use serde::{Deserialize, Serialize};

use crate::copy_storage::{decode_record, query_statement};
use crate::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RetireReceipt {
    pub transfer_id: TransferId,
    pub fence: Epoch,
    pub acked: ReplicationPosition,
    pub retired_at: DateTime<Utc>,
}

fn invalid_retirement() -> CentralError {
    StoreError::Corrupt {
        field: "copy retirement".into(),
    }
    .into()
}

pub(crate) fn read_retirement(tx: &mut dyn SqlTx) -> Result<Option<RetireReceipt>, CentralError> {
    let columns = tx.query(query_statement(
        "SELECT name FROM pragma_table_info('_copy_state')",
        Vec::new(),
        ["name"],
    ))?;
    let names = columns
        .iter()
        .map(|row| row.text("name"))
        .collect::<Result<Vec<_>, _>>()?;
    let marker_columns = ["retired", "retire_receipt"];
    let present = marker_columns
        .iter()
        .filter(|name| names.contains(name))
        .count();
    if present == 0 {
        return Ok(None);
    }
    if present != marker_columns.len() {
        return Err(invalid_retirement());
    }
    let rows = tx.query(query_statement(
        "SELECT retired,retire_receipt FROM _copy_state WHERE singleton=1",
        Vec::new(),
        ["retired", "retire_receipt"],
    ))?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    match row.integer("retired")? {
        0 if matches!(
            row.value("retire_receipt")?,
            collaboration_replication_protocol::SqlValue::Null
        ) =>
        {
            Ok(None)
        }
        1 => decode_record(row.text("retire_receipt")?, "retire receipt").map(Some),
        _ => Err(invalid_retirement()),
    }
}

pub(crate) fn answer_retired(
    input: CopyInput,
    retired: RetireReceipt,
) -> Result<Vec<CopyEffect>, CentralError> {
    let (call_id, result) = match input {
        CopyInput::Health { call_id, .. } => (
            call_id,
            CopyResult::Health(Box::new(CopyHealth::Retired {
                transfer: retired.transfer_id,
                retired_at: retired.retired_at,
            })),
        ),
        CopyInput::Call { call_id, call, .. } => {
            let result = match call.as_ref() {
                CopyCall::Ops(CopyOpsCall::Health {}) => {
                    return Err(CentralError::InvalidInput {
                        message: "Health requires the target shell's measurement".into(),
                    });
                }
                CopyCall::Ops(CopyOpsCall::Migrate {}) => {
                    CopyResult::Migrated(CopyMigrationResult::Retired {})
                }
                CopyCall::ApplyPiece { .. } => CopyResult::ApplyPiece(
                    collaboration_replication_protocol::SessionAnswer::Refused(
                        collaboration_replication_protocol::SessionRefusal::Retired,
                    ),
                ),
                CopyCall::SnapshotPage { .. } => CopyResult::SnapshotPage(
                    collaboration_replication_protocol::SessionAnswer::ReadPage(
                        collaboration_replication_protocol::ReadPage::ProjectCopy(
                            collaboration_replication_protocol::ProjectCopyRead::Retired,
                        ),
                    ),
                ),
                CopyCall::ImportSeed { .. } => CopyResult::ImportSeed(ImportSeedResult {
                    answer: collaboration_replication_protocol::ProjectImportAnswer::Retired,
                    unheld: None,
                }),
                _ => CopyResult::Retired,
            };
            (call_id, result)
        }
        CopyInput::Alarm | CopyInput::CallResult { .. } | CopyInput::CallFailed { .. } => {
            return Ok(Vec::new());
        }
    };
    Ok(vec![CopyEffect::Return { call_id, result }])
}
