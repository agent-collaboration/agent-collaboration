//! Ordered, transactional copy bookkeeping changes. The shell owns rollback.
use serde::{Deserialize, Serialize};

use crate::copy_storage::{query_statement, statement, text};
use crate::{CopyBookkeepingMetadata, RegistryTime, SqlTx, read_copy_bookkeeping_metadata};

pub const COPY_BOOKKEEPING_VERSION: u64 = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CopyMigrationReply {
    NoReply,
    Call(crate::ObjectCallId),
    Migrate(crate::ObjectCallId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CopyMigrationResult {
    Retired {},
    Migrated { from: u64, to: u64 },
    Current { version: u64 },
    Failed { version: u64, reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("copy bookkeeping migration {version}: {cause}")]
pub struct CopyMigrationFailure {
    pub version: u64,
    #[source]
    cause: CopyMigrationCause,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
enum CopyMigrationCause {
    #[error(transparent)]
    Storage(crate::StoreError),
    #[error("{0}")]
    Invalid(String),
}

impl CopyMigrationFailure {
    #[must_use]
    pub fn is_full(&self) -> bool {
        matches!(
            self.cause,
            CopyMigrationCause::Storage(crate::StoreError::Full)
        )
    }
    #[must_use]
    pub fn reason(&self) -> String {
        self.cause.to_string()
    }
}

pub fn migrate_copy_bookkeeping(
    tx: &mut dyn SqlTx,
    now: RegistryTime,
) -> Result<CopyMigrationResult, CopyMigrationFailure> {
    let metadata = read_copy_bookkeeping_metadata(tx).map_err(|error| failed_core(1, error))?;
    let from = match metadata {
        CopyBookkeepingMetadata::Uninitialized => 0,
        CopyBookkeepingMetadata::Unrecorded { shape_v1: true } => 1,
        CopyBookkeepingMetadata::Unrecorded { shape_v1: false } => {
            return Err(failed(1, "unrecorded bookkeeping does not match v1"));
        }
        CopyBookkeepingMetadata::Recorded { version } if version == COPY_BOOKKEEPING_VERSION => {
            return Ok(CopyMigrationResult::Current { version });
        }
        CopyBookkeepingMetadata::Recorded { version } if version > COPY_BOOKKEEPING_VERSION => {
            return Err(failed(
                version,
                "bookkeeping version is newer than supported",
            ));
        }
        CopyBookkeepingMetadata::Recorded { version } => version,
    };
    if from == 0 {
        execute_batch(tx, include_str!("../migrations/0001_copy_storage.sql"))
            .map_err(|error| failed_store(1, error))?;
    }
    tx.execute(statement("CREATE TABLE IF NOT EXISTS _copy_migrations(version INTEGER PRIMARY KEY,applied_at TEXT NOT NULL,from_version INTEGER NOT NULL)", Vec::new())).map_err(|error| failed_store(1, error))?;
    if !matches!(metadata, CopyBookkeepingMetadata::Recorded { .. }) {
        record_version(tx, 1, from, now)?;
    }
    if from < 2 {
        execute_batch(tx, include_str!("../migrations/0012_copy_operations.sql"))
            .map_err(|error| failed_store(2, error))?;
        if !crate::copy_bookkeeping::directory_outbox_shape_matches(tx)
            .map_err(|error| failed_core(2, error))?
        {
            return Err(failed(
                2,
                "directory outbox does not match the operations schema",
            ));
        }
        crate::copy_piece_time::backfill_piece_time(tx).map_err(|error| failed_core(2, error))?;
        record_version(tx, 2, 1, now)?;
    }
    // A prior failure is diagnostic debt only until the migration succeeds.
    if !tx
        .query(query_statement(
            "SELECT name FROM sqlite_schema WHERE type='table' AND name='_copy_migration_failure'",
            Vec::new(),
            ["name"],
        ))
        .map_err(|error| failed_store(COPY_BOOKKEEPING_VERSION, error))?
        .is_empty()
    {
        tx.execute(statement("DELETE FROM _copy_migration_failure", Vec::new()))
            .map_err(|error| failed_store(COPY_BOOKKEEPING_VERSION, error))?;
    }
    Ok(CopyMigrationResult::Migrated {
        from,
        to: COPY_BOOKKEEPING_VERSION,
    })
}

fn record_version(
    tx: &mut dyn SqlTx,
    version: u64,
    from_version: u64,
    now: RegistryTime,
) -> Result<(), CopyMigrationFailure> {
    tx.execute(statement(
        "INSERT INTO _copy_migrations(version,applied_at,from_version) VALUES(?,?,?)",
        vec![
            collaboration_replication_protocol::SqlValue::Integer(
                i64::try_from(version).map_err(|error| failed(version, error))?,
            ),
            text(now.0.to_rfc3339()),
            collaboration_replication_protocol::SqlValue::Integer(
                i64::try_from(from_version).map_err(|error| failed(version, error))?,
            ),
        ],
    ))
    .map_err(|error| failed_store(version, error))?;
    Ok(())
}

fn execute_batch(tx: &mut dyn SqlTx, batch: &str) -> Result<(), crate::StoreError> {
    for sql in batch
        .split(';')
        .map(str::trim)
        .filter(|sql| !sql.is_empty())
    {
        tx.execute(statement(sql, Vec::new()))?;
    }
    Ok(())
}

fn failed(version: u64, reason: impl std::fmt::Display) -> CopyMigrationFailure {
    CopyMigrationFailure {
        version,
        cause: CopyMigrationCause::Invalid(reason.to_string()),
    }
}

fn failed_store(version: u64, error: crate::StoreError) -> CopyMigrationFailure {
    CopyMigrationFailure {
        version,
        cause: CopyMigrationCause::Storage(error),
    }
}
fn failed_core(version: u64, error: crate::CentralError) -> CopyMigrationFailure {
    match error {
        crate::CentralError::Store(error) => failed_store(version, error),
        error => failed(version, error),
    }
}

/// Called only inside the shell's separate transaction after event rollback.
pub fn record_copy_migration_failure(
    tx: &mut dyn SqlTx,
    failure: &CopyMigrationFailure,
    now: RegistryTime,
) -> Result<(), crate::CentralError> {
    tx.execute(statement("CREATE TABLE IF NOT EXISTS _copy_migration_failure(version INTEGER NOT NULL,reason TEXT NOT NULL,at TEXT NOT NULL)", Vec::new()))?;
    tx.execute(statement("DELETE FROM _copy_migration_failure", Vec::new()))?;
    tx.execute(statement(
        "INSERT INTO _copy_migration_failure(version,reason,at) VALUES(?,?,?)",
        vec![
            collaboration_replication_protocol::SqlValue::Integer(
                i64::try_from(failure.version).map_err(|_| crate::StoreError::Corrupt {
                    field: "copy migration failure version".into(),
                })?,
            ),
            text(failure.reason()),
            text(now.0.to_rfc3339()),
        ],
    ))?;
    Ok(())
}
