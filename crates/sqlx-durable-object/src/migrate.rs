//! SQLx migrations inside a Durable Object's open storage transaction.
//!
//! [`run_migrations`] hands SQLx's stock [`Migrator`] a [`Migrate`] adapter over a
//! [`DoTransaction`]. SQLx alone decides which versions are pending, compares
//! checksums and refuses dirty, unknown or changed history; the adapter reads and
//! validates the stored history, runs the one migration SQLx asks for and writes its
//! receipt. Nothing here begins or commits: the caller's
//! [`DoConnection::transaction`](crate::DoConnection::transaction) does, so a failed
//! migration leaves neither its SQL nor its receipt.
//!
//! Carried from agent-fleet-sync c217f0b (`durable-steps::scope_migrations`) without
//! durable-steps. The history table and its receipts are SQLx SQLite's own:
//! `installed_on` is the column default and `execution_time` is `-1`, the receipt
//! stock SQLx SQLite writes before it records the duration.
//!
//! SQLx does not enforce every rule a history relies on, so the adapter also refuses,
//! before SQLx traverses anything: a source that is not strictly ordered or contains a
//! `no_tx` migration; a stored receipt whose fields lack their storage classes; and an
//! unapplied source version below the highest applied one.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::time::Duration;

use futures_core::future::BoxFuture;
use sqlx_core::migrate::{AppliedMigration, Migrate, MigrateError, Migration, Migrator};
use sqlx_core::query::query;
use sqlx_core::raw_sql::raw_sql;
use sqlx_core::row::Row;
use sqlx_core::sql_str::AssertSqlSafe;

use crate::{DoRow, DoTransaction};

/// What one successful run did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationProgress {
    /// Versions this run applied.
    pub applied: usize,
    /// The highest version the stored history held before the run.
    pub from: Option<i64>,
    /// The source's highest up version; `None` for an empty source.
    pub to: Option<i64>,
}

/// Why a run stopped. The caller's transaction must roll back on every variant.
#[derive(Debug, thiserror::Error)]
pub enum MigrationRunError {
    /// SQLx refused the history: dirty, unknown or changed versions.
    #[error("SQLx refused the migration history: {0}")]
    Policy(MigrateError),
    /// The source or the stored history broke a rule SQLx does not check.
    #[error(transparent)]
    History(HistoryFault),
    /// SQLx asked to apply `version` and it failed.
    #[error("migration {version} failed: {cause}")]
    AtVersion {
        version: i64,
        cause: MigrationVersionCause,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum MigrationVersionCause {
    /// The migration's own SQL.
    #[error("its SQL failed: {0}")]
    Statement(sqlx_core::Error),
    /// Writing the receipt after its SQL succeeded.
    #[error("its receipt could not be written: {0}")]
    Receipt(sqlx_core::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryFault {
    /// Creating or reading the history table failed.
    #[error("the migration history could not be read or created: {0}")]
    Storage(sqlx_core::Error),
    /// The configured history table is not a plain SQL identifier.
    #[error("the migration history table is not a plain identifier")]
    TableName,
    /// `version` does not follow its predecessor strictly.
    #[error("migration {version} is out of order in its source")]
    Unordered { version: i64 },
    /// `version` asks to run outside the transaction.
    #[error("migration {version} must run inside the transaction")]
    NoTransaction { version: i64 },
    /// A stored receipt field lacks its storage class or value.
    #[error("a migration receipt has an invalid {column}")]
    Record { column: &'static str },
    #[error("migration {0} has two receipts")]
    DuplicateVersion(i64),
    /// `version` was never applied, but `highest` was.
    #[error("migration {version} is missing below applied migration {highest}")]
    LowerHole { version: i64, highest: i64 },
}

/// Runs `migrator` inside `transaction`.
///
/// # Errors
///
/// Any [`MigrationRunError`]; the caller must let its transaction roll back.
pub async fn run_migrations(
    transaction: &DoTransaction,
    migrator: &Migrator,
) -> Result<MigrationProgress, MigrationRunError> {
    if !is_plain_identifier(&migrator.table_name) {
        return Err(MigrationRunError::History(HistoryFault::TableName));
    }
    let to = validate_source(migrator).map_err(MigrationRunError::History)?;
    let mut adapter = TransactionMigrate {
        transaction,
        source: migrator,
        history: None,
        from: None,
        applied: 0,
        fault: None,
    };
    let outcome = migrator.run_direct(None, &mut adapter, false).await;
    match (adapter.fault.take(), outcome) {
        (Some(fault), _) => Err(fault),
        (None, Err(error)) => Err(MigrationRunError::Policy(error)),
        (None, Ok(())) => Ok(MigrationProgress {
            applied: adapter.applied,
            from: adapter.from,
            to,
        }),
    }
}

fn up_versions(migrator: &Migrator) -> impl Iterator<Item = &Migration> {
    migrator
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
}

fn validate_source(migrator: &Migrator) -> Result<Option<i64>, HistoryFault> {
    let mut previous: Option<i64> = None;
    for migration in up_versions(migrator) {
        if migration.no_tx {
            return Err(HistoryFault::NoTransaction {
                version: migration.version,
            });
        }
        if previous.is_some_and(|version| migration.version <= version) {
            return Err(HistoryFault::Unordered {
                version: migration.version,
            });
        }
        previous = Some(migration.version);
    }
    Ok(previous)
}

/// `[A-Za-z_][A-Za-z0-9_]*`: library- and domain-owned table names only.
fn is_plain_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// SQLx SQLite's history DDL, so a table created here matches one the stock
/// migrator creates on a native SQLite file.
fn history_table_sql(table: &str) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS {table} (\n    version BIGINT PRIMARY KEY,\n    description TEXT NOT NULL,\n    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,\n    success BOOLEAN NOT NULL,\n    checksum BLOB NOT NULL,\n    execution_time BIGINT NOT NULL\n);\n"
    )
}

fn history_read_sql(table: &str) -> String {
    format!(
        "SELECT CAST(version AS INTEGER) AS version, CAST(success AS INTEGER) AS success, checksum, \
         typeof(version) AS version_class, typeof(success) AS success_class, \
         typeof(checksum) AS checksum_class FROM {table} ORDER BY version"
    )
}

/// A receipt that passed validation; `dirty` is SQLx's to judge.
struct ValidReceipt {
    version: i64,
    checksum: Vec<u8>,
    dirty: bool,
}

fn validate_receipt(row: &DoRow) -> Result<ValidReceipt, HistoryFault> {
    let text = |column: &'static str| -> Result<Option<String>, HistoryFault> {
        row.try_get(column)
            .map_err(|_| HistoryFault::Record { column })
    };
    let class_is = |column: &'static str, expected: &str| -> Result<bool, HistoryFault> {
        Ok(text(column)?.as_deref() == Some(expected))
    };
    if !class_is("version_class", "integer")? {
        return Err(HistoryFault::Record { column: "version" });
    }
    let version: i64 = row
        .try_get("version")
        .map_err(|_| HistoryFault::Record { column: "version" })?;
    if !class_is("success_class", "integer")? {
        return Err(HistoryFault::Record { column: "success" });
    }
    // 1 is applied; 0 is a dirty record SQLx refuses; nothing else is a receipt.
    let dirty = match row.try_get::<i64, _>("success") {
        Ok(1) => false,
        Ok(0) => true,
        _ => return Err(HistoryFault::Record { column: "success" }),
    };
    if !class_is("checksum_class", "blob")? {
        return Err(HistoryFault::Record { column: "checksum" });
    }
    let checksum: Vec<u8> = row
        .try_get("checksum")
        .map_err(|_| HistoryFault::Record { column: "checksum" })?;
    Ok(ValidReceipt {
        version,
        checksum,
        dirty,
    })
}

/// The adapter SQLx drives, for one [`run_migrations`] call inside the caller's
/// open transaction; that exclusivity is why `lock` and `unlock` do nothing.
struct TransactionMigrate<'run> {
    transaction: &'run DoTransaction,
    source: &'run Migrator,
    history: Option<Vec<ValidReceipt>>,
    from: Option<i64>,
    applied: usize,
    /// The original cause when the adapter, not SQLx, stopped the run.
    fault: Option<MigrationRunError>,
}

/// What the adapter returns to SQLx after recording its own cause in `fault`.
fn stopped() -> MigrateError {
    MigrateError::Execute(sqlx_core::Error::Protocol(
        "the migration run stopped; its cause is returned by run_migrations".into(),
    ))
}

impl TransactionMigrate<'_> {
    async fn history(&mut self, table: &str) -> Result<&[ValidReceipt], MigrateError> {
        if self.history.is_none() {
            match self.read_history(table).await {
                Ok(history) => {
                    self.from = history.last().map(|receipt| receipt.version);
                    self.history = Some(history);
                }
                Err(fault) => {
                    self.fault = Some(MigrationRunError::History(fault));
                    return Err(stopped());
                }
            }
        }
        Ok(self.history.as_deref().unwrap_or_default())
    }

    async fn read_history(&mut self, table: &str) -> Result<Vec<ValidReceipt>, HistoryFault> {
        let rows = query(AssertSqlSafe(history_read_sql(table)))
            .fetch_all(self.transaction)
            .await
            .map_err(HistoryFault::Storage)?;
        let mut history = Vec::with_capacity(rows.len());
        let mut versions = BTreeSet::new();
        for row in &rows {
            let receipt = validate_receipt(row)?;
            if !versions.insert(receipt.version) {
                return Err(HistoryFault::DuplicateVersion(receipt.version));
            }
            history.push(receipt);
        }
        // Stock SQLx would apply a source version below the highest applied one.
        // Only versions the source knows count: an applied version missing from
        // the source is SQLx's `VersionMissing` to refuse.
        if let Some(highest) = up_versions(self.source)
            .map(|migration| migration.version)
            .filter(|version| versions.contains(version))
            .max()
            && let Some(hole) = up_versions(self.source)
                .map(|migration| migration.version)
                .find(|version| *version < highest && !versions.contains(version))
        {
            return Err(HistoryFault::LowerHole {
                version: hole,
                highest,
            });
        }
        Ok(history)
    }
}

impl Migrate for TransactionMigrate<'_> {
    fn create_schema_if_not_exists<'e>(
        &'e mut self,
        schema_name: &'e str,
    ) -> BoxFuture<'e, Result<(), MigrateError>> {
        // SQLite has no schemas to create.
        Box::pin(async move {
            Err(MigrateError::CreateSchemasNotSupported(
                schema_name.to_owned(),
            ))
        })
    }

    fn ensure_migrations_table<'e>(
        &'e mut self,
        table_name: &'e str,
    ) -> BoxFuture<'e, Result<(), MigrateError>> {
        Box::pin(async move {
            if let Err(error) = raw_sql(AssertSqlSafe(history_table_sql(table_name)))
                .execute(self.transaction)
                .await
            {
                self.fault = Some(MigrationRunError::History(HistoryFault::Storage(error)));
                return Err(stopped());
            }
            Ok(())
        })
    }

    fn dirty_version<'e>(
        &'e mut self,
        table_name: &'e str,
    ) -> BoxFuture<'e, Result<Option<i64>, MigrateError>> {
        Box::pin(async move {
            Ok(self
                .history(table_name)
                .await?
                .iter()
                .find(|receipt| receipt.dirty)
                .map(|receipt| receipt.version))
        })
    }

    fn list_applied_migrations<'e>(
        &'e mut self,
        table_name: &'e str,
    ) -> BoxFuture<'e, Result<Vec<AppliedMigration>, MigrateError>> {
        Box::pin(async move {
            Ok(self
                .history(table_name)
                .await?
                .iter()
                .map(|receipt| AppliedMigration {
                    version: receipt.version,
                    checksum: Cow::Owned(receipt.checksum.clone()),
                })
                .collect())
        })
    }

    fn lock(&mut self) -> BoxFuture<'_, Result<(), MigrateError>> {
        Box::pin(async { Ok(()) })
    }

    fn unlock(&mut self) -> BoxFuture<'_, Result<(), MigrateError>> {
        Box::pin(async { Ok(()) })
    }

    fn apply<'e>(
        &'e mut self,
        table_name: &'e str,
        migration: &'e Migration,
    ) -> BoxFuture<'e, Result<Duration, MigrateError>> {
        Box::pin(async move {
            let version = migration.version;
            if let Err(error) = raw_sql(migration.sql.clone())
                .execute(self.transaction)
                .await
            {
                self.fault = Some(MigrationRunError::AtVersion {
                    version,
                    cause: MigrationVersionCause::Statement(error),
                });
                return Err(stopped());
            }
            let written = query(AssertSqlSafe(format!(
                "INSERT INTO {table_name}(version, description, success, checksum, execution_time) VALUES (?, ?, TRUE, ?, -1)"
            )))
            .bind(version)
            .bind(migration.description.to_string())
            .bind(migration.checksum.to_vec())
            .execute(self.transaction)
            .await;
            match written {
                Ok(_) => {
                    self.applied = self.applied.saturating_add(1);
                    Ok(Duration::ZERO)
                }
                Err(error) => {
                    self.fault = Some(MigrationRunError::AtVersion {
                        version,
                        cause: MigrationVersionCause::Receipt(error),
                    });
                    Err(stopped())
                }
            }
        })
    }

    fn revert<'e>(
        &'e mut self,
        _table_name: &'e str,
        migration: &'e Migration,
    ) -> BoxFuture<'e, Result<Duration, MigrateError>> {
        // `run_migrations` never asks SQLx to undo; histories are append-only.
        let version = migration.version;
        Box::pin(async move {
            Err(MigrateError::ExecuteMigration(
                sqlx_core::Error::Protocol("down migrations are not supported".into()),
                version,
            ))
        })
    }
}

#[cfg(test)]
#[path = "migrate_test.rs"]
mod tests;
