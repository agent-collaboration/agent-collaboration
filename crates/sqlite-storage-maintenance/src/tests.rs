use std::{
    error::Error,
    io,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

use crate::{
    AvailableSpaceProbe, DatabasePreparationDeferral, ExistingDatabasePreparation,
    configure_new_database_options, incremental_vacuum_step, prepare_existing_database_with_probe,
};

struct FixedAvailableSpaceProbe {
    available_bytes: io::Result<u64>,
    calls: AtomicUsize,
}

impl FixedAvailableSpaceProbe {
    fn with_space(available_bytes: u64) -> Self {
        Self {
            available_bytes: Ok(available_bytes),
            calls: AtomicUsize::new(0),
        }
    }

    fn with_probe_error() -> Self {
        Self {
            available_bytes: Err(io::Error::other("probe unavailable")),
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl AvailableSpaceProbe for FixedAvailableSpaceProbe {
    fn available_bytes(&self, _directory: &Path) -> io::Result<u64> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.available_bytes {
            Ok(available_bytes) => Ok(*available_bytes),
            Err(error) => Err(io::Error::new(error.kind(), error.to_string())),
        }
    }
}

fn options_for(path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
}

async fn create_rows(connection: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    sqlx::query("CREATE TABLE retained_rows(payload BLOB NOT NULL)")
        .execute(&mut *connection)
        .await?;
    sqlx::query(
        "WITH RECURSIVE row_number(value) AS (
             VALUES (1)
             UNION ALL
             SELECT value + 1 FROM row_number WHERE value < 600
         )
         INSERT INTO retained_rows(payload)
         SELECT zeroblob(4096) FROM row_number",
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

#[tokio::test]
async fn new_database_enables_incremental_auto_vacuum_before_first_table()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("new.sqlite");
    let options = configure_new_database_options(options_for(&database_path), &database_path)?;
    let mut connection = SqliteConnection::connect_with(&options).await?;

    let mode: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut connection)
        .await?;
    if mode != 2 {
        return Err(format!("new database auto-vacuum mode was {mode}; expected 2").into());
    }
    create_rows(&mut connection).await?;
    let mode_after_schema: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut connection)
        .await?;
    if mode_after_schema != 2 {
        return Err(format!(
            "auto-vacuum mode after schema creation was {mode_after_schema}; expected 2"
        )
        .into());
    }
    Ok(())
}

#[tokio::test]
async fn existing_database_converts_once_and_skips_space_probe_afterward()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("existing.sqlite");
    let options = options_for(&database_path).journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    create_rows(&mut connection).await?;
    sqlx::query("DELETE FROM retained_rows")
        .execute(&mut connection)
        .await?;
    let checkpoint_busy: i64 = sqlx::query_scalar("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(&mut connection)
        .await?;
    if checkpoint_busy != 0 {
        return Err(format!("pre-conversion WAL checkpoint was busy: {checkpoint_busy}").into());
    }
    let main_file_before = std::fs::metadata(&database_path)?.len();
    let database_and_wal_before = crate::sqlite_database_and_wal_size(&database_path)?;
    let required_bytes = database_and_wal_before.saturating_mul(2);
    let probe = FixedAvailableSpaceProbe::with_space(required_bytes);

    let first_preparation =
        prepare_existing_database_with_probe(&mut connection, &database_path, &probe).await?;
    if !matches!(
        &first_preparation,
        ExistingDatabasePreparation::Converted {
            before_bytes,
            after_bytes: Some(after_bytes),
            post_checkpoint: crate::WalCheckpointOutcome::Completed,
            ..
        } if *before_bytes == database_and_wal_before
            && *after_bytes < *before_bytes
    ) {
        return Err(format!(
            "expected conversion to shrink main+WAL after TRUNCATE, observed {first_preparation:?}"
        )
        .into());
    }
    let main_file_after = std::fs::metadata(&database_path)?.len();
    if main_file_after >= main_file_before {
        return Err(format!(
            "conversion did not shrink the main file: before={main_file_before}, after={main_file_after}"
        )
        .into());
    }
    if probe.calls() != 1 {
        return Err(format!(
            "available-space probe called {} times; expected once",
            probe.calls()
        )
        .into());
    }
    let mode: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut connection)
        .await?;
    if mode != 2 {
        return Err(format!("converted database auto-vacuum mode was {mode}; expected 2").into());
    }

    let second_probe = FixedAvailableSpaceProbe::with_space(u64::MAX);
    let second_preparation =
        prepare_existing_database_with_probe(&mut connection, &database_path, &second_probe)
            .await?;
    if second_preparation != ExistingDatabasePreparation::AlreadyIncremental {
        return Err(format!(
            "second preparation was {second_preparation:?}; expected AlreadyIncremental"
        )
        .into());
    }
    if second_probe.calls() != 0 {
        return Err(format!(
            "second available-space probe called {} times; expected zero",
            second_probe.calls()
        )
        .into());
    }
    Ok(())
}

#[tokio::test]
async fn existing_database_conversion_defers_when_space_is_below_twice_file_size()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("low-space.sqlite");
    let mut connection = SqliteConnection::connect_with(&options_for(&database_path)).await?;
    sqlx::query("CREATE TABLE retained_rows(payload BLOB NOT NULL)")
        .execute(&mut connection)
        .await?;
    let required_bytes = std::fs::metadata(&database_path)?.len().saturating_mul(2);
    let available_bytes = required_bytes.saturating_sub(1);
    let probe = FixedAvailableSpaceProbe::with_space(available_bytes);

    let preparation =
        prepare_existing_database_with_probe(&mut connection, &database_path, &probe).await?;
    let expected_preparation =
        ExistingDatabasePreparation::Deferred(DatabasePreparationDeferral::InsufficientSpace {
            required_bytes,
            available_bytes,
        });
    if preparation != expected_preparation {
        return Err(format!(
            "low-space preparation was {preparation:?}; expected {expected_preparation:?}"
        )
        .into());
    }
    if probe.calls() != 1 {
        return Err(format!(
            "available-space probe called {} times; expected once",
            probe.calls()
        )
        .into());
    }
    let mode: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut connection)
        .await?;
    if mode != 0 {
        return Err(format!("deferred database auto-vacuum mode was {mode}; expected 0").into());
    }
    Ok(())
}

#[tokio::test]
async fn existing_database_conversion_defers_when_space_probe_fails() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("probe-error.sqlite");
    let mut connection = SqliteConnection::connect_with(&options_for(&database_path)).await?;
    sqlx::query("CREATE TABLE retained_rows(payload BLOB NOT NULL)")
        .execute(&mut connection)
        .await?;
    let probe = FixedAvailableSpaceProbe::with_probe_error();

    let preparation =
        prepare_existing_database_with_probe(&mut connection, &database_path, &probe).await?;
    let expected_preparation = ExistingDatabasePreparation::Deferred(
        DatabasePreparationDeferral::AvailableSpaceUnavailable,
    );
    if preparation != expected_preparation {
        return Err(format!(
            "failed-probe preparation was {preparation:?}; expected {expected_preparation:?}"
        )
        .into());
    }
    if probe.calls() != 1 {
        return Err(format!(
            "available-space probe called {} times; expected once",
            probe.calls()
        )
        .into());
    }
    let mode: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut connection)
        .await?;
    if mode != 0 {
        return Err(format!("deferred database auto-vacuum mode was {mode}; expected 0").into());
    }
    Ok(())
}

#[tokio::test]
async fn busy_preconversion_checkpoint_defers_without_failing_or_waiting()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("busy-checkpoint.sqlite");
    let options = options_for(&database_path).journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    let mut writer = SqliteConnection::connect_with(&options).await?;
    sqlx::query("CREATE TABLE retained_rows(payload TEXT NOT NULL)")
        .execute(&mut writer)
        .await?;
    sqlx::query("INSERT INTO retained_rows(payload) VALUES ('before snapshot')")
        .execute(&mut writer)
        .await?;

    let mut reader = SqliteConnection::connect_with(&options).await?;
    let mut read_transaction = reader.begin().await?;
    let _: i64 = sqlx::query_scalar("SELECT count(*) FROM retained_rows")
        .fetch_one(&mut *read_transaction)
        .await?;
    sqlx::query("INSERT INTO retained_rows(payload) VALUES ('after snapshot')")
        .execute(&mut writer)
        .await?;
    let busy_timeout_before: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut writer)
        .await?;
    let probe = FixedAvailableSpaceProbe::with_space(u64::MAX);

    let preparation =
        prepare_existing_database_with_probe(&mut writer, &database_path, &probe).await?;

    let expected_preparation =
        ExistingDatabasePreparation::Deferred(DatabasePreparationDeferral::WalCheckpointBusy);
    if preparation != expected_preparation {
        return Err(format!(
            "busy-checkpoint preparation was {preparation:?}; expected {expected_preparation:?}"
        )
        .into());
    }
    if probe.calls() != 0 {
        return Err(format!(
            "available-space probe called {} times; expected zero",
            probe.calls()
        )
        .into());
    }
    let mode: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut writer)
        .await?;
    if mode != 0 {
        return Err(format!("deferred database auto-vacuum mode was {mode}; expected 0").into());
    }
    let busy_timeout_after: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut writer)
        .await?;
    if busy_timeout_after != busy_timeout_before {
        return Err(format!(
            "conversion changed busy timeout: before={busy_timeout_before}, after={busy_timeout_after}"
        )
        .into());
    }
    read_transaction.rollback().await?;
    Ok(())
}

#[tokio::test]
async fn incremental_vacuum_removes_at_most_one_bounded_page_batch() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("incremental.sqlite");
    let options = configure_new_database_options(
        options_for(&database_path).journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
        &database_path,
    )?;
    let mut connection = SqliteConnection::connect_with(&options).await?;
    create_rows(&mut connection).await?;
    sqlx::query("DELETE FROM retained_rows")
        .execute(&mut connection)
        .await?;
    let checkpoint_busy: i64 = sqlx::query_scalar("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(&mut connection)
        .await?;
    if checkpoint_busy != 0 {
        return Err(format!("pre-vacuum WAL checkpoint was busy: {checkpoint_busy}").into());
    }
    let main_file_before = std::fs::metadata(&database_path)?.len();
    let freelist_before: i64 = sqlx::query_scalar("PRAGMA freelist_count")
        .fetch_one(&mut connection)
        .await?;
    if freelist_before <= i64::from(crate::INCREMENTAL_VACUUM_PAGE_LIMIT) {
        return Err(format!(
            "vacuum fixture freelist had {freelist_before} pages; expected more than {}",
            crate::INCREMENTAL_VACUUM_PAGE_LIMIT
        )
        .into());
    }
    let page_count_before: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(&mut connection)
        .await?;

    incremental_vacuum_step(&mut connection).await?;

    let page_count_after: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(&mut connection)
        .await?;
    let freelist_after: i64 = sqlx::query_scalar("PRAGMA freelist_count")
        .fetch_one(&mut connection)
        .await?;
    let pages_removed = page_count_before - page_count_after;
    if pages_removed <= 0 {
        return Err(format!(
            "incremental vacuum removed {pages_removed} pages; expected a positive count"
        )
        .into());
    }
    if pages_removed > i64::from(crate::INCREMENTAL_VACUUM_PAGE_LIMIT) {
        return Err(format!(
            "incremental vacuum removed {pages_removed} pages; limit is {}",
            crate::INCREMENTAL_VACUUM_PAGE_LIMIT
        )
        .into());
    }
    if freelist_after <= 0 {
        return Err(format!(
            "incremental vacuum left {freelist_after} free pages; expected a remainder"
        )
        .into());
    }
    let main_file_after = std::fs::metadata(&database_path)?.len();
    if main_file_after >= main_file_before {
        return Err(format!(
            "incremental vacuum did not shrink the main file: before={main_file_before}, after={main_file_after}"
        )
        .into());
    }
    Ok(())
}
