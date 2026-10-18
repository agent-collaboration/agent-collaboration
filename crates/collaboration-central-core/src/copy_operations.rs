//! Owner operation contracts and schema-tolerant copy diagnostics.
use chrono::{DateTime, Utc};
use collaboration_replication_protocol::{Epoch, TransactionKey};
use serde::{Deserialize, Serialize};

use crate::copy_storage::query_statement;
use crate::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CopyOpsCall {
    Health {},
    Repair {},
    Migrate {},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CopyAdminFailure {
    Retired {},
    CentralCopyFull,
    CopyUnavailable {
        reason: collaboration_replication_protocol::CopyUnavailableReason,
    },
}
impl CopyAdminFailure {
    #[must_use]
    pub fn from_reason(reason: collaboration_replication_protocol::CopyUnavailableReason) -> Self {
        match reason {
            collaboration_replication_protocol::CopyUnavailableReason::StorageFull => {
                Self::CentralCopyFull
            }
            reason => Self::CopyUnavailable { reason },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum CopyHealthRead<TValue> {
    Known(TValue),
    Unreadable { detail: String },
}
impl<TValue> CopyHealthRead<TValue> {
    fn from_read(read: Result<TValue, CentralError>) -> Self {
        match read {
            Ok(value) => Self::Known(value),
            Err(error) => Self::Unreadable {
                detail: error.to_string(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyMigrationFailureRecord {
    pub version: u64,
    pub reason: String,
    pub at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StagedPartHealth {
    pub transaction: TransactionKey,
    pub index: u32,
    pub staged_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LiveCopyHealth {
    pub bookkeeping: CopyHealthRead<CopyBookkeepingMetadata>,
    pub supported_bookkeeping_version: u64,
    pub migration_failure: CopyHealthRead<Option<CopyMigrationFailureRecord>>,
    pub copy: CopyHealthRead<Option<CopyStatus>>,
    /// Last newly accepted piece or staged part. Replays do not advance it.
    /// Older copies retain the best timestamp their bookkeeping still records.
    pub last_piece_at: CopyHealthRead<Option<DateTime<Utc>>>,
    pub project_fingerprint: CopyHealthRead<String>,
    pub staged_parts: CopyHealthRead<Vec<StagedPartHealth>>,
    pub storage_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CopyHealth {
    Live(Box<LiveCopyHealth>),
    Retired {
        transfer: collaboration_replication_protocol::TransferId,
        retired_at: DateTime<Utc>,
    },
}

pub(crate) fn read_copy_health(tx: &mut dyn SqlTx, storage_bytes: u64) -> CopyHealth {
    let retirement = crate::copy_retirement::read_retirement(tx);
    if let Ok(Some(retired)) = retirement {
        return CopyHealth::Retired {
            transfer: retired.transfer_id,
            retired_at: retired.retired_at,
        };
    }
    let copy = retirement.and_then(|_| read_copy(tx));
    CopyHealth::Live(Box::new(LiveCopyHealth {
        bookkeeping: CopyHealthRead::from_read(read_copy_bookkeeping_metadata(tx)),
        supported_bookkeeping_version: COPY_BOOKKEEPING_VERSION,
        migration_failure: CopyHealthRead::from_read(read_failure(tx)),
        copy: CopyHealthRead::from_read(copy),
        last_piece_at: CopyHealthRead::from_read(crate::copy_piece_time::read_piece_time(tx)),
        project_fingerprint: CopyHealthRead::from_read(crate::row_apply::schema_fingerprint(tx)),
        staged_parts: CopyHealthRead::from_read(read_parts(tx)),
        storage_bytes,
    }))
}

fn read_copy(tx: &mut dyn SqlTx) -> Result<Option<CopyStatus>, CentralError> {
    if has_table(tx, "_copy_state")? {
        crate::copy_storage::load_copy(tx)
    } else {
        Ok(None)
    }
}

fn has_table(tx: &mut dyn SqlTx, table: &str) -> Result<bool, CentralError> {
    crate::registry_dispatch::has_table(tx, table)
}
fn corrupt(field: &str) -> CentralError {
    StoreError::Corrupt {
        field: field.into(),
    }
    .into()
}
fn timestamp(value: &str, field: &str) -> Result<DateTime<Utc>, CentralError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| corrupt(field))
}
fn read_failure(tx: &mut dyn SqlTx) -> Result<Option<CopyMigrationFailureRecord>, CentralError> {
    if !has_table(tx, "_copy_migration_failure")? {
        return Ok(None);
    }
    let rows = tx.query(query_statement(
        "SELECT version,reason,at FROM _copy_migration_failure",
        Vec::new(),
        ["version", "reason", "at"],
    ))?;
    if rows.len() > 1 {
        return Err(corrupt("copy migration failure cardinality"));
    }
    rows.first()
        .map(|row| {
            Ok(CopyMigrationFailureRecord {
                version: u64::try_from(row.integer("version")?)
                    .ok()
                    .filter(|version| *version > 0)
                    .ok_or_else(|| corrupt("copy migration failure version"))?,
                reason: row.text("reason")?.into(),
                at: timestamp(row.text("at")?, "copy migration failure time")?,
            })
        })
        .transpose()
}
fn read_parts(tx: &mut dyn SqlTx) -> Result<Vec<StagedPartHealth>, CentralError> {
    if !has_table(tx, "_copy_staged_parts")? {
        return Ok(Vec::new());
    }
    tx.query(query_statement("SELECT transaction_epoch,transaction_commit,part_index,staged_at FROM _copy_staged_parts ORDER BY transaction_epoch,transaction_commit,part_index",Vec::new(),["transaction_epoch","transaction_commit","part_index","staged_at"]))?.iter().map(|row|Ok(StagedPartHealth {
        transaction:TransactionKey { epoch:Epoch::new(row.unsigned("transaction_epoch")?).map_err(|_|corrupt("staged epoch"))?,commit_id:row.unsigned("transaction_commit")?.into() },
        index:u32::try_from(row.integer("part_index")?).map_err(|_|corrupt("staged part index"))?,
        staged_at:timestamp(row.text("staged_at")?,"staged part time")?,
    })).collect()
}
