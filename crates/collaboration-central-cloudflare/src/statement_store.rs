//! Runs core's statement batches and reads on a Durable Object's SQL storage.
//!
//! A batch runs inside `Storage::transaction`: every statement, its guard, and for
//! central's own object the `SetFromQuery` alarm statement and `setAlarm`, commit
//! together or roll back together. The store knows no business table.

use crate::store_wire::{OwnedAlarmUpdate, OwnedBatch, OwnedStatement};
use central_storage::{
    BatchOutcome, ConstraintKind, CorruptReason, ReadOutcome, SafeInteger, SqlValue, StoreFailure,
};
use sqlx_core::column::Column;
use sqlx_core::error::ErrorKind;
use sqlx_core::migrate::Migrator;
use sqlx_core::query::{Query, query};
use sqlx_core::row::Row;
use sqlx_core::sql_str::AssertSqlSafe;
use sqlx_durable_object::{Do, DoArguments, DoConnection, DoRow, DoTransaction};
use sqlx_durable_object_core::{AsValue, Value};
use std::sync::Arc;

/// Whether a batch's `AlarmUpdate` reaches the object's alarm.
pub(crate) enum AlarmPolicy {
    /// Central's own object: `SetFromQuery` sets or clears its one alarm, through
    /// this handle on the same object's storage.
    Apply(worker::Storage),
    /// A workspace store owns no alarm; the update is ignored.
    Ignore,
}

/// Why a batch's transaction rolled back.
enum BatchStop {
    GuardFailed { index: u32, rows_affected: u64 },
    ConstraintViolated { index: u32, kind: ConstraintKind },
    Failed(StoreFailure),
}

impl From<sqlx_core::Error> for BatchStop {
    fn from(error: sqlx_core::Error) -> Self {
        Self::Failed(store_failure(&error))
    }
}

/// A database error that is not a constraint: the store could not do what it was asked.
fn store_failure(error: &sqlx_core::Error) -> StoreFailure {
    worker::console_warn!("statement store failure: {error}");
    StoreFailure::Unavailable
}

fn constraint_kind(error: &sqlx_core::Error) -> Option<ConstraintKind> {
    let sqlx_core::Error::Database(database) = error else {
        return None;
    };
    match database.kind() {
        ErrorKind::UniqueViolation => Some(ConstraintKind::Unique),
        ErrorKind::ForeignKeyViolation => Some(ConstraintKind::ForeignKey),
        ErrorKind::NotNullViolation => Some(ConstraintKind::NotNull),
        // Decision 31: CHECK constraints exist only on boolean columns.
        ErrorKind::CheckViolation => Some(ConstraintKind::CheckBoolean),
        _ => None,
    }
}

fn bound(statement: &OwnedStatement) -> Query<'static, Do, DoArguments> {
    statement.args.iter().fold(
        query(AssertSqlSafe(statement.sql.clone())),
        |query, argument| match argument {
            SqlValue::Null => query.bind(None::<i64>),
            SqlValue::Integer(value) => query.bind(value.get()),
            SqlValue::Text(value) => query.bind(value.clone()),
            SqlValue::Blob(value) => query.bind(value.clone()),
        },
    )
}

fn sql_value(value: &Value, column: &str) -> Result<SqlValue, StoreFailure> {
    match value {
        Value::Null => Ok(SqlValue::Null),
        // The driver already refuses wider integers; a value outside ±(2^53−1) here
        // is still refused rather than trusted.
        Value::Integer(value) => SafeInteger::new(*value)
            .map(SqlValue::Integer)
            .map_err(|_| StoreFailure::Corrupt {
                reason: CorruptReason::BadValue {
                    column: column.to_owned(),
                },
            }),
        // REAL is outside the storage contract (no schema stores one).
        Value::Text(value) => Ok(SqlValue::Text(value.clone())),
        Value::Blob(value) => Ok(SqlValue::Blob(value.clone())),
        _ => Err(StoreFailure::Corrupt {
            reason: CorruptReason::UnknownKind {
                column: column.to_owned(),
            },
        }),
    }
}

fn read_outcome(rows: &[DoRow]) -> Result<ReadOutcome, StoreFailure> {
    let columns: Vec<String> = rows
        .first()
        .map(|row| {
            row.columns()
                .iter()
                .map(|column| column.name().to_owned())
                .collect()
        })
        .unwrap_or_default();
    let rows = rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .enumerate()
                .map(|(index, column)| {
                    let raw = row.try_get_raw(index).map_err(|_| StoreFailure::Corrupt {
                        reason: CorruptReason::MissingColumn {
                            column: column.clone(),
                        },
                    })?;
                    sql_value(raw.as_value(), column)
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ReadOutcome { columns, rows })
}

/// The alarm `SetFromQuery` asks for: one INTEGER Unix-millisecond value, or NULL.
fn alarm_time(outcome: &ReadOutcome) -> Result<Option<i64>, StoreFailure> {
    let column = outcome.columns.first().cloned().unwrap_or_default();
    let bad_value = || StoreFailure::Corrupt {
        reason: CorruptReason::BadValue {
            column: column.clone(),
        },
    };
    match outcome.rows.as_slice() {
        [row] => match row.as_slice() {
            [SqlValue::Integer(at)] => Ok(Some(at.get())),
            [SqlValue::Null] => Ok(None),
            _ => Err(bad_value()),
        },
        _ => Err(bad_value()),
    }
}

fn alarm_out_of_range(rows: &[DoRow]) -> StoreFailure {
    let column = rows
        .first()
        .and_then(|row| row.columns().first())
        .map(|column| column.name().to_owned())
        .unwrap_or_default();
    StoreFailure::Corrupt {
        reason: CorruptReason::BadValue { column },
    }
}

async fn run_in_transaction(
    transaction: &DoTransaction,
    batch: OwnedBatch,
    policy: AlarmPolicy,
) -> Result<Vec<u64>, BatchStop> {
    let mut rows_affected = Vec::with_capacity(batch.statements.len());
    for (index, guarded) in batch.statements.iter().enumerate() {
        let index =
            u32::try_from(index).map_err(|_| BatchStop::Failed(StoreFailure::Unavailable))?;
        let affected = match bound(&guarded.statement).execute(transaction).await {
            Ok(result) => result.rows_affected(),
            Err(error) => {
                return Err(match constraint_kind(&error) {
                    Some(kind) => BatchStop::ConstraintViolated { index, kind },
                    None => BatchStop::Failed(store_failure(&error)),
                });
            }
        };
        if !guarded.expect.admits(affected) {
            return Err(BatchStop::GuardFailed {
                index,
                rows_affected: affected,
            });
        }
        rows_affected.push(affected);
    }
    if let (AlarmPolicy::Apply(storage), OwnedAlarmUpdate::SetFromQuery(statement)) =
        (policy, &batch.alarm)
    {
        let rows = bound(statement).fetch_all(transaction).await?;
        let at = alarm_time(&read_outcome(&rows).map_err(BatchStop::Failed)?)
            .map_err(BatchStop::Failed)?;
        // A storage write in the same transaction: it commits with the batch or not at all.
        let written = match at {
            Some(at) => match chrono::DateTime::from_timestamp_millis(at) {
                Some(at) => storage.set_alarm(at).await,
                None => return Err(BatchStop::Failed(alarm_out_of_range(&rows))),
            },
            None => storage.delete_alarm().await,
        };
        written.map_err(|error| {
            worker::console_warn!("statement store alarm write failed: {error}");
            BatchStop::Failed(StoreFailure::Unavailable)
        })?;
    }
    Ok(rows_affected)
}

/// Runs `batch` atomically on `connection`'s storage.
pub(crate) async fn run_batch(
    connection: &DoConnection,
    batch: OwnedBatch,
    policy: AlarmPolicy,
) -> Result<BatchOutcome, StoreFailure> {
    let outcome = connection
        .transaction(move |transaction| async move {
            run_in_transaction(&transaction, batch, policy).await
        })
        .await;
    match outcome {
        Ok(rows_affected) => Ok(BatchOutcome::Applied { rows_affected }),
        Err(BatchStop::GuardFailed {
            index,
            rows_affected,
        }) => Ok(BatchOutcome::GuardFailed {
            index,
            rows_affected,
        }),
        Err(BatchStop::ConstraintViolated { index, kind }) => {
            Ok(BatchOutcome::ConstraintViolated { index, kind })
        }
        Err(BatchStop::Failed(failure)) => Err(failure),
    }
}

/// Runs one read outside any write.
pub(crate) async fn read(
    connection: &DoConnection,
    statement: OwnedStatement,
) -> Result<ReadOutcome, StoreFailure> {
    let rows = bound(&statement)
        .fetch_all(connection)
        .await
        .map_err(|error| store_failure(&error))?;
    read_outcome(&rows)
}

/// Why opening an object's store failed; its transaction rolled back.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct MigrationFailure(String);

/// Runs `migrator` inside one storage transaction: all pending migrations and their
/// receipts commit together, or none do.
pub(crate) async fn migrate(
    connection: &DoConnection,
    migrator: Arc<Migrator>,
) -> Result<(), MigrationFailure> {
    connection
        .transaction(move |transaction| async move {
            sqlx_durable_object::migrate::run_migrations(&transaction, &migrator)
                .await
                .map(|_| ())
                .map_err(|error| MigrationFailure(error.to_string()))
        })
        .await
}

impl From<sqlx_core::Error> for MigrationFailure {
    fn from(error: sqlx_core::Error) -> Self {
        Self(error.to_string())
    }
}

#[cfg(test)]
#[path = "statement_store_test.rs"]
mod tests;
