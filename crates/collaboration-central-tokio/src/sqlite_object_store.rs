//! One object's SQLite file on stock SQLx: runs core's statement batches atomically.
//!
//! The file is opened with foreign keys on and the object kind's SQLx migrator run before
//! the first batch. Every batch runs in one `BEGIN IMMEDIATE` transaction; a missed guard
//! or a constraint error rolls the whole batch back. The store knows no business table.

use collaboration_central_core::storage::{
    AlarmUpdate, BatchOutcome, ConstraintKind, CorruptReason, ReadOutcome, SafeInteger, SqlValue,
    Statement, StatementBatch, StoreFailure,
};
use runtime_primitives::SyncTime;
use sqlx::error::ErrorKind;
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::{
    SqliteArguments, SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
    SqliteRow,
};
use sqlx::{Arguments, Column, Row, SqliteConnection, TypeInfo, ValueRef};
use std::path::Path;

/// What a committed batch asks of the object's alarm.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlarmChange {
    Unchanged,
    Set(SyncTime),
    Clear,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreOpenFailure {
    #[error("the object's SQLite file could not be opened")]
    Open(#[source] sqlx::Error),
    #[error("the object's migrations did not apply")]
    Migrate(#[source] MigrateError),
}

pub struct SqliteObjectStore {
    pool: SqlitePool,
}

impl SqliteObjectStore {
    /// Opens (creating if missing) the object's file and runs its migrator before any batch.
    pub async fn open_and_migrate(
        path: &Path,
        migrator: &Migrator,
    ) -> Result<Self, StoreOpenFailure> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        // One connection: the object is the only writer, and batches never interleave.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(StoreOpenFailure::Open)?;
        if let Err(failure) = migrator.run(&pool).await {
            pool.close().await;
            return Err(StoreOpenFailure::Migrate(failure));
        }
        Ok(Self { pool })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn run_batch(
        &self,
        batch: StatementBatch,
    ) -> Result<(BatchOutcome, AlarmChange), StoreFailure> {
        let mut transaction = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|_| StoreFailure::Unavailable)?;
        let mut rows_affected = Vec::with_capacity(batch.statements.len());
        for (index, guarded) in batch.statements.into_iter().enumerate() {
            let index = u32::try_from(index).map_err(|_| StoreFailure::Unavailable)?;
            let arguments = bind_arguments(guarded.statement.args)?;
            let affected = match execute(&mut transaction, guarded.statement.sql, arguments).await {
                Ok(affected) => affected,
                Err(sqlx::Error::Database(error)) => {
                    let Some(kind) = constraint_kind(error.kind()) else {
                        return Err(StoreFailure::Unavailable);
                    };
                    transaction
                        .rollback()
                        .await
                        .map_err(|_| StoreFailure::Unavailable)?;
                    return Ok((
                        BatchOutcome::ConstraintViolated { index, kind },
                        AlarmChange::Unchanged,
                    ));
                }
                Err(_) => return Err(StoreFailure::Unavailable),
            };
            if !guarded.expect.admits(affected) {
                transaction
                    .rollback()
                    .await
                    .map_err(|_| StoreFailure::Unavailable)?;
                return Ok((
                    BatchOutcome::GuardFailed {
                        index,
                        rows_affected: affected,
                    },
                    AlarmChange::Unchanged,
                ));
            }
            rows_affected.push(affected);
        }
        let alarm = match batch.alarm {
            AlarmUpdate::Unchanged => AlarmChange::Unchanged,
            AlarmUpdate::SetFromQuery(query) => alarm_from_query(&mut transaction, query).await?,
        };
        transaction
            .commit()
            .await
            .map_err(|_| StoreFailure::AnswerLost)?;
        Ok((BatchOutcome::Applied { rows_affected }, alarm))
    }

    pub async fn read(&self, statement: Statement) -> Result<ReadOutcome, StoreFailure> {
        let mut connection = self
            .pool
            .acquire()
            .await
            .map_err(|_| StoreFailure::Unavailable)?;
        read_rows(&mut connection, statement).await
    }
}

fn bind_arguments(args: Vec<SqlValue>) -> Result<SqliteArguments, StoreFailure> {
    let mut arguments = SqliteArguments::default();
    for value in args {
        let bound = match value {
            SqlValue::Null => arguments.add(None::<i64>),
            SqlValue::Integer(value) => arguments.add(value.get()),
            SqlValue::Text(value) => arguments.add(value),
            SqlValue::Blob(value) => arguments.add(value),
        };
        bound.map_err(|_| StoreFailure::Unavailable)?;
    }
    Ok(arguments)
}

async fn execute(
    connection: &mut SqliteConnection,
    sql: &'static str,
    arguments: SqliteArguments,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query_with(sql, arguments).execute(connection).await?;
    Ok(result.rows_affected())
}

async fn read_rows(
    connection: &mut SqliteConnection,
    statement: Statement,
) -> Result<ReadOutcome, StoreFailure> {
    let arguments = bind_arguments(statement.args)?;
    let rows = sqlx::query_with(statement.sql, arguments)
        .fetch_all(connection)
        .await
        .map_err(|_| StoreFailure::Unavailable)?;
    let columns = match rows.first() {
        Some(row) => row
            .columns()
            .iter()
            .map(|column| column.name().to_owned())
            .collect(),
        None => Vec::new(),
    };
    let rows = rows
        .iter()
        .map(|row| decode_row(row, &columns))
        .collect::<Result<_, _>>()?;
    Ok(ReadOutcome { columns, rows })
}

fn decode_row(row: &SqliteRow, columns: &[String]) -> Result<Vec<SqlValue>, StoreFailure> {
    columns
        .iter()
        .enumerate()
        .map(|(index, column)| decode_value(row, index, column))
        .collect()
}

fn decode_value(row: &SqliteRow, index: usize, column: &str) -> Result<SqlValue, StoreFailure> {
    let corrupt = || StoreFailure::Corrupt {
        reason: CorruptReason::BadValue {
            column: column.to_owned(),
        },
    };
    let raw = row.try_get_raw(index).map_err(|_| corrupt())?;
    if raw.is_null() {
        return Ok(SqlValue::Null);
    }
    let storage_class = raw.type_info().name().to_owned();
    match storage_class.as_str() {
        "INTEGER" => {
            let value: i64 = row.try_get(index).map_err(|_| corrupt())?;
            // Both hosts keep ±(2^53−1); a wider stored integer is corrupt, never truncated.
            SafeInteger::new(value)
                .map(SqlValue::Integer)
                .map_err(|_| corrupt())
        }
        "TEXT" => row
            .try_get(index)
            .map(SqlValue::Text)
            .map_err(|_| corrupt()),
        "BLOB" => row
            .try_get(index)
            .map(SqlValue::Blob)
            .map_err(|_| corrupt()),
        // REAL is outside the contract (no schema stores one); refuse it rather than guess.
        _ => Err(StoreFailure::Corrupt {
            reason: CorruptReason::UnknownKind {
                column: column.to_owned(),
            },
        }),
    }
}

/// Runs the Timers query last in the batch's transaction; one INTEGER (ms) or NULL.
async fn alarm_from_query(
    connection: &mut SqliteConnection,
    query: Statement,
) -> Result<AlarmChange, StoreFailure> {
    let answer = read_rows(connection, query).await?;
    let column = answer.columns.first().cloned().unwrap_or_default();
    let bad_value = || StoreFailure::Corrupt {
        reason: CorruptReason::BadValue {
            column: column.clone(),
        },
    };
    match answer.rows.as_slice() {
        [row] => match row.as_slice() {
            [SqlValue::Null] => Ok(AlarmChange::Clear),
            [SqlValue::Integer(milliseconds)] => Ok(AlarmChange::Set(SyncTime::from_unix_millis(
                milliseconds.get(),
            ))),
            _ => Err(bad_value()),
        },
        _ => Err(bad_value()),
    }
}

const fn constraint_kind(kind: ErrorKind) -> Option<ConstraintKind> {
    match kind {
        ErrorKind::UniqueViolation => Some(ConstraintKind::Unique),
        ErrorKind::ForeignKeyViolation => Some(ConstraintKind::ForeignKey),
        ErrorKind::NotNullViolation => Some(ConstraintKind::NotNull),
        // Decision 31: CHECK exists only on boolean columns.
        ErrorKind::CheckViolation => Some(ConstraintKind::CheckBoolean),
        _ => None,
    }
}
