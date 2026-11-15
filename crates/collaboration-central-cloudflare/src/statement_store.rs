//! Runs core's statement batches and reads on a Durable Object's SQL storage.
//!
//! A batch runs inside `Storage::transaction`: every statement, its guard, and for
//! central's own object the `SetFromQuery` alarm statement and `setAlarm`, commit
//! together or roll back together. The store knows no business table.

use crate::store_wire::{OwnedAlarmUpdate, OwnedBatch, OwnedStatement};
use central_storage::{
    AlarmPublication, AlarmPublicationFailure, BatchOutcome, ConstraintKind, CorruptReason,
    EngineAlarmDecider, EngineAlarmTimes, ReadOutcome, SafeInteger, SqlValue, StoreFailure,
};
use runtime_primitives::Clock;
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
    Engine(
        worker::Storage,
        crate::worker_clock::WorkspaceClock,
        EngineAlarmDecider,
    ),
    /// A workspace store owns no alarm; the update is ignored.
    Ignore,
}

/// Why a batch's transaction rolled back.
enum BatchStop {
    GuardFailed { index: u32, rows_affected: u64 },
    ConstraintViolated { index: u32, kind: ConstraintKind },
    StatementRejected { index: u32 },
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
) -> Result<(Vec<u64>, AlarmPublication), BatchStop> {
    let mut rows_affected = Vec::with_capacity(batch.statements.len());
    for (index, guarded) in batch.statements.iter().enumerate() {
        let index =
            u32::try_from(index).map_err(|_| BatchStop::Failed(StoreFailure::Unavailable))?;
        let affected = match bound(&guarded.statement).execute(transaction).await {
            Ok(result) => result.rows_affected(),
            Err(error) => {
                return Err(match constraint_kind(&error) {
                    Some(kind) => BatchStop::ConstraintViolated { index, kind },
                    None if statement_is_rejected(&error) => BatchStop::StatementRejected { index },
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
    let (storage, clock) = match policy {
        AlarmPolicy::Apply(storage) => (Some(storage), None),
        AlarmPolicy::Engine(storage, clock, decide) => (Some(storage), Some((clock, decide))),
        AlarmPolicy::Ignore => (None, None),
    };
    let mut publication = AlarmPublication::Unchanged;
    if let Some(storage) = storage {
        let at = match &batch.alarm {
            OwnedAlarmUpdate::Unchanged => return Ok((rows_affected, publication)),
            OwnedAlarmUpdate::SetFromQuery(statement) => {
                let rows = bound(statement).fetch_all(transaction).await?;
                let at = alarm_time(&read_outcome(&rows).map_err(BatchStop::Failed)?)
                    .map_err(BatchStop::Failed)?;
                if at.is_some_and(|at| chrono::DateTime::from_timestamp_millis(at).is_none()) {
                    return Err(BatchStop::Failed(alarm_out_of_range(&rows)));
                }
                at
            }
            OwnedAlarmUpdate::SetEngineFromQuery { query } => {
                let rows = bound(query).fetch_all(transaction).await?;
                let answer = read_outcome(&rows).map_err(BatchStop::Failed)?;
                let (clock, decide) = clock.ok_or_else(|| {
                    BatchStop::Failed(StoreFailure::AlarmUnpublishable {
                        reason: AlarmPublicationFailure::ClockUnavailable,
                    })
                })?;
                decided_engine_alarm(&answer, clock.now().unix_millis(), &decide)
                    .map_err(BatchStop::Failed)?
            }
        };
        // A storage write in the same transaction: it commits with the batch or not at all.
        let written = match at {
            Some(at) => match chrono::DateTime::from_timestamp_millis(at) {
                Some(at) => storage.set_alarm(at).await,
                None => {
                    return Err(BatchStop::Failed(StoreFailure::AlarmUnpublishable {
                        reason: AlarmPublicationFailure::EarliestDueOutOfRange,
                    }));
                }
            },
            None => storage.delete_alarm().await,
        };
        written.map_err(|error| {
            worker::console_warn!("statement store alarm write failed: {error}");
            BatchStop::Failed(StoreFailure::Unavailable)
        })?;
        publication = at.map_or(AlarmPublication::Cleared, |unix_millis| {
            AlarmPublication::Set { unix_millis }
        });
    }
    Ok((rows_affected, publication))
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
        Ok((rows_affected, publication)) => Ok(BatchOutcome::Applied {
            rows_affected,
            publication,
        }),
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
        Err(BatchStop::StatementRejected { index }) => {
            Ok(BatchOutcome::StatementRejected { index })
        }
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

fn statement_is_rejected(error: &sqlx_core::Error) -> bool {
    let sqlx_core::Error::Database(database) = error else {
        return false;
    };
    matches!(
        database.code().as_deref(),
        Some(
            "SQLITE_CONSTRAINT"
                | "SQLITE_MISMATCH"
                | "SQLITE_CONSTRAINT_COMMITHOOK"
                | "SQLITE_CONSTRAINT_FUNCTION"
                | "SQLITE_CONSTRAINT_TRIGGER"
                | "SQLITE_CONSTRAINT_VTAB"
                | "SQLITE_CONSTRAINT_ROWID"
                | "SQLITE_CONSTRAINT_PINNED"
                | "SQLITE_CONSTRAINT_DATATYPE"
        )
    )
}

fn decided_engine_alarm(
    answer: &ReadOutcome,
    now: i64,
    decide: &EngineAlarmDecider,
) -> Result<Option<i64>, StoreFailure> {
    let bad = || StoreFailure::Corrupt {
        reason: CorruptReason::BadValue {
            column: "engine alarm input".into(),
        },
    };
    let value = |value: &SqlValue| -> Result<Option<i64>, StoreFailure> {
        match value {
            SqlValue::Null => Ok(None),
            SqlValue::Integer(time) => Ok(Some(time.get())),
            SqlValue::Text(_) | SqlValue::Blob(_) => Err(bad()),
        }
    };
    let (grid, legacy) = match answer.rows.as_slice() {
        [row] => match row.as_slice() {
            [grid, legacy] => (value(grid)?, value(legacy)?),
            _ => return Err(bad()),
        },
        _ => return Err(bad()),
    };
    decide(
        EngineAlarmTimes {
            earliest_on_grid_due_unix_millis: grid,
            earliest_legacy_due_unix_millis: legacy,
        },
        now,
        &mut |_| {},
    )
    .map_err(|reason| StoreFailure::AlarmUnpublishable { reason })
}
