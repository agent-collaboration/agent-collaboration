//! Shared incremental auto-vacuum preparation for Router-owned SQLite files.

use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    time::Instant,
};

use sqlx::{Row, SqliteConnection, sqlite::SqliteConnectOptions};
use thiserror::Error;

/// Maximum number of database pages removed by one incremental maintenance step.
pub const INCREMENTAL_VACUUM_PAGE_LIMIT: u32 = 64;

/// Result of preparing an existing SQLite database for incremental auto-vacuum.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExistingDatabasePreparation {
    /// The database already uses incremental auto-vacuum.
    AlreadyIncremental,
    /// The database was converted with one full `VACUUM`.
    Converted {
        /// Main database file size before conversion.
        before_bytes: u64,
        /// Main database file size after conversion, when it could be read.
        after_bytes: Option<u64>,
        /// Elapsed time for the conversion.
        elapsed: std::time::Duration,
        /// Outcome of the post-conversion WAL checkpoint.
        post_checkpoint: WalCheckpointOutcome,
    },
    /// Conversion was deferred because the available disk space was not established as safe.
    Deferred(DatabasePreparationDeferral),
}

/// Reason an existing database's auto-vacuum conversion was deferred.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatabasePreparationDeferral {
    /// The main database file size could not be read.
    DatabaseSizeUnavailable,
    /// The filesystem free-space probe failed.
    AvailableSpaceUnavailable,
    /// A WAL checkpoint could not finish because another connection held the database.
    WalCheckpointBusy,
    /// The full vacuum could not run; conversion will be retried on the next open.
    VacuumUnavailable,
    /// Available space was below twice the combined main database and WAL size.
    InsufficientSpace {
        /// Required free bytes, equal to twice the combined main file and WAL size.
        required_bytes: u64,
        /// Available bytes reported by the filesystem probe.
        available_bytes: u64,
    },
}

/// Outcome of an optional WAL checkpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WalCheckpointOutcome {
    /// The database is not using WAL mode, so no checkpoint was needed.
    NotWal,
    /// The checkpoint completed without a busy reader or writer.
    Completed,
    /// The checkpoint reported that another connection prevented completion.
    Busy,
}

#[derive(Clone, Copy)]
enum WalCheckpointMode {
    Truncate,
    Passive,
}

/// SQLite storage maintenance failure.
#[derive(Debug, Error)]
pub enum SqliteStorageMaintenanceError {
    /// SQLite rejected a maintenance statement.
    #[error("SQLite storage maintenance failed")]
    Sqlite(#[from] sqlx::Error),
    /// The database path could not be classified before opening.
    #[error("unable to inspect SQLite database path")]
    DatabasePath(#[from] io::Error),
    /// SQLite did not report incremental auto-vacuum after conversion.
    #[error("SQLite database did not enable incremental auto-vacuum")]
    IncrementalAutoVacuumNotEnabled,
}

/// Supplies available filesystem space for an existing SQLite database's conversion guard.
pub trait AvailableSpaceProbe: Send + Sync {
    /// Returns available bytes on the filesystem containing `directory`.
    fn available_bytes(&self, directory: &Path) -> io::Result<u64>;
}

/// Filesystem-backed available-space probe used by production store openers.
#[derive(Clone, Copy, Debug, Default)]
pub struct FilesystemAvailableSpaceProbe;

impl AvailableSpaceProbe for FilesystemAvailableSpaceProbe {
    fn available_bytes(&self, directory: &Path) -> io::Result<u64> {
        fs2::available_space(directory)
    }
}

/// Adds the incremental auto-vacuum pragma before SQLite creates a new or empty database.
pub fn configure_new_database_options(
    options: SqliteConnectOptions,
    database_path: &Path,
) -> Result<SqliteConnectOptions, SqliteStorageMaintenanceError> {
    let is_new_or_empty = match fs::metadata(database_path) {
        Ok(metadata) => metadata.len() == 0,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => return Err(error.into()),
    };

    Ok(if is_new_or_empty {
        options.pragma("auto_vacuum", "INCREMENTAL")
    } else {
        options
    })
}

/// Converts an existing database once, provided twice its file size is available on disk.
pub async fn prepare_existing_database(
    connection: &mut SqliteConnection,
    database_path: &Path,
) -> Result<ExistingDatabasePreparation, SqliteStorageMaintenanceError> {
    prepare_existing_database_with_probe(connection, database_path, &FilesystemAvailableSpaceProbe)
        .await
}

/// Converts an existing database using an injected available-space probe.
pub async fn prepare_existing_database_with_probe(
    connection: &mut SqliteConnection,
    database_path: &Path,
    space_probe: &dyn AvailableSpaceProbe,
) -> Result<ExistingDatabasePreparation, SqliteStorageMaintenanceError> {
    let auto_vacuum_mode: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut *connection)
        .await?;
    if auto_vacuum_mode == 2 {
        return Ok(ExistingDatabasePreparation::AlreadyIncremental);
    }

    let pre_checkpoint = wal_checkpoint_truncate(connection).await?;
    if pre_checkpoint == WalCheckpointOutcome::Busy {
        tracing::warn!(
            database_path = ?database_path,
            checkpoint = "before_conversion",
            "sqlite_storage_maintenance.conversion_deferred"
        );
        return Ok(ExistingDatabasePreparation::Deferred(
            DatabasePreparationDeferral::WalCheckpointBusy,
        ));
    }

    let before_bytes = match sqlite_database_and_wal_size(database_path) {
        Ok(size) => size,
        Err(error) => {
            tracing::warn!(
                database_path = ?database_path,
                error_kind = ?error.kind(),
                "sqlite_storage_maintenance.conversion_deferred"
            );
            return Ok(ExistingDatabasePreparation::Deferred(
                DatabasePreparationDeferral::DatabaseSizeUnavailable,
            ));
        }
    };
    let required_bytes = before_bytes.saturating_mul(2);
    let directory = database_directory(database_path);
    let available_bytes = match space_probe.available_bytes(directory) {
        Ok(available_bytes) => available_bytes,
        Err(error) => {
            tracing::warn!(
                database_path = ?database_path,
                directory = ?directory,
                error_kind = ?error.kind(),
                "sqlite_storage_maintenance.conversion_deferred"
            );
            return Ok(ExistingDatabasePreparation::Deferred(
                DatabasePreparationDeferral::AvailableSpaceUnavailable,
            ));
        }
    };
    if available_bytes < required_bytes {
        tracing::warn!(
            database_path = ?database_path,
            database_size_bytes = before_bytes,
            available_space_bytes = available_bytes,
            required_space_bytes = required_bytes,
            "sqlite_storage_maintenance.conversion_deferred"
        );
        return Ok(ExistingDatabasePreparation::Deferred(
            DatabasePreparationDeferral::InsufficientSpace {
                required_bytes,
                available_bytes,
            },
        ));
    }
    let started_at = Instant::now();
    tracing::info!(
        database_path = ?database_path,
        database_and_wal_size_before_bytes = before_bytes,
        "sqlite_storage_maintenance.conversion_started"
    );
    sqlx::query("PRAGMA auto_vacuum = INCREMENTAL")
        .execute(&mut *connection)
        .await?;
    if let Err(error) = vacuum_without_busy_wait(connection).await {
        tracing::warn!(
            database_path = ?database_path,
            error_kind = sqlite_error_class(&error),
            "sqlite_storage_maintenance.conversion_deferred"
        );
        return Ok(ExistingDatabasePreparation::Deferred(
            DatabasePreparationDeferral::VacuumUnavailable,
        ));
    }

    let converted_mode: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut *connection)
        .await?;
    if converted_mode != 2 {
        return Err(SqliteStorageMaintenanceError::IncrementalAutoVacuumNotEnabled);
    }

    let post_checkpoint = wal_checkpoint_truncate(connection).await?;
    if post_checkpoint == WalCheckpointOutcome::Busy {
        tracing::warn!(
            database_path = ?database_path,
            checkpoint = "after_conversion",
            "sqlite_storage_maintenance.checkpoint_busy"
        );
    }
    let elapsed = started_at.elapsed();
    let after_bytes = sqlite_database_and_wal_size(database_path).ok();
    tracing::info!(
        database_path = ?database_path,
        database_and_wal_size_before_bytes = before_bytes,
        database_and_wal_size_after_bytes = after_bytes,
        elapsed_milliseconds = elapsed.as_millis(),
        "sqlite_storage_maintenance.conversion_completed"
    );
    Ok(ExistingDatabasePreparation::Converted {
        before_bytes,
        after_bytes,
        elapsed,
        post_checkpoint,
    })
}

async fn vacuum_without_busy_wait(connection: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    let previous_busy_timeout_ms: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *connection)
        .await?;
    sqlx::query("PRAGMA busy_timeout=0")
        .execute(&mut *connection)
        .await?;
    let vacuum_result = sqlx::query("VACUUM")
        .execute(&mut *connection)
        .await
        .map(|_result| ());
    let restore_timeout_result = set_busy_timeout(connection, previous_busy_timeout_ms).await;
    match vacuum_result {
        Ok(()) => restore_timeout_result,
        Err(error) => {
            restore_timeout_result?;
            Err(error)
        }
    }
}

/// Performs one bounded incremental-vacuum step, removing at most 64 pages.
pub async fn incremental_vacuum_step(connection: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    sqlx::query("PRAGMA incremental_vacuum(64)")
        .execute(&mut *connection)
        .await?;
    if wal_checkpoint_passive(connection).await? == WalCheckpointOutcome::Busy {
        tracing::warn!(
            checkpoint = "after_incremental_vacuum",
            "sqlite_storage_maintenance.passive_checkpoint_incomplete"
        );
    }
    Ok(())
}

async fn wal_checkpoint_truncate(
    connection: &mut SqliteConnection,
) -> Result<WalCheckpointOutcome, sqlx::Error> {
    checkpoint_wal(connection, WalCheckpointMode::Truncate, true).await
}

async fn wal_checkpoint_passive(
    connection: &mut SqliteConnection,
) -> Result<WalCheckpointOutcome, sqlx::Error> {
    checkpoint_wal(connection, WalCheckpointMode::Passive, false).await
}

async fn checkpoint_wal(
    connection: &mut SqliteConnection,
    mode: WalCheckpointMode,
    nonblocking: bool,
) -> Result<WalCheckpointOutcome, sqlx::Error> {
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut *connection)
        .await?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Ok(WalCheckpointOutcome::NotWal);
    }

    let previous_busy_timeout_ms: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *connection)
        .await?;
    if nonblocking {
        sqlx::query("PRAGMA busy_timeout=0")
            .execute(&mut *connection)
            .await?;
    }

    let checkpoint_result = match mode {
        WalCheckpointMode::Truncate => {
            sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                .fetch_one(&mut *connection)
                .await
        }
        WalCheckpointMode::Passive => {
            sqlx::query("PRAGMA wal_checkpoint(PASSIVE)")
                .fetch_one(&mut *connection)
                .await
        }
    }
    .and_then(|row| row.try_get::<i64, _>("busy"));

    let restore_timeout_result = if nonblocking {
        set_busy_timeout(connection, previous_busy_timeout_ms).await
    } else {
        Ok(())
    };
    match checkpoint_result {
        Ok(0) => {
            restore_timeout_result?;
            Ok(WalCheckpointOutcome::Completed)
        }
        Ok(_) => {
            restore_timeout_result?;
            Ok(WalCheckpointOutcome::Busy)
        }
        Err(error) if is_busy_sqlite_error(&error) => {
            restore_timeout_result?;
            Ok(WalCheckpointOutcome::Busy)
        }
        Err(error) => {
            restore_timeout_result?;
            Err(error)
        }
    }
}

async fn set_busy_timeout(
    connection: &mut SqliteConnection,
    timeout_milliseconds: i64,
) -> Result<(), sqlx::Error> {
    let statement = sqlx::AssertSqlSafe(format!("PRAGMA busy_timeout={timeout_milliseconds}"));
    sqlx::query(statement).execute(&mut *connection).await?;
    Ok(())
}

fn sqlite_database_and_wal_size(database_path: &Path) -> io::Result<u64> {
    let database_bytes = fs::metadata(database_path)?.len();
    let wal_bytes = match fs::metadata(sqlite_sidecar_path(database_path, "-wal")) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    Ok(database_bytes.saturating_add(wal_bytes))
}

fn sqlite_sidecar_path(database_path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = OsString::from(database_path.as_os_str());
    sidecar.push(suffix);
    PathBuf::from(sidecar)
}

fn sqlite_error_class(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::Database(_) if is_busy_sqlite_error(error) => "busy_or_locked",
        sqlx::Error::Database(_) => "database",
        sqlx::Error::Io(_) => "io",
        _ => "other",
    }
}

fn is_busy_sqlite_error(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(database_error) = error else {
        return false;
    };
    matches!(database_error.code().as_deref(), Some("5" | "6"))
        || database_error.message().contains("database is locked")
        || database_error
            .message()
            .contains("database table is locked")
}

fn database_directory(database_path: &Path) -> &Path {
    database_path
        .parent()
        .filter(|directory| !directory.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[cfg(test)]
mod tests;
