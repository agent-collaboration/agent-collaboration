use super::{JournalError, ObservationJournal, connect_journal};
use collaboration_protocol::UuidIdentity;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{cell::Cell, error::Error, time::Duration};

fn identity() -> Result<UuidIdentity, Box<dyn Error>> {
    Ok("00000000-0000-7000-8000-000000000001"
        .to_owned()
        .try_into()?)
}

#[tokio::test]
async fn successful_open_keeps_the_full_native_busy_timeout() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let mut journal =
        ObservationJournal::open(&directory.path().join("journal.sqlite"), identity()?).await?;

    let busy_timeout_ms: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut journal.connection)
        .await?;

    if busy_timeout_ms != 1000 {
        return Err(format!("journal busy timeout changed to {busy_timeout_ms}ms").into());
    }
    journal.close().await;
    Ok(())
}

#[tokio::test]
async fn persistent_read_lock_returns_the_original_busy_error() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.sqlite");
    let mut reader = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await?;
    sqlx::query("CREATE TABLE held_read(value INTEGER)")
        .execute(&mut reader)
        .await?;
    let mut transaction = reader.begin().await?;
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM held_read")
        .fetch_one(&mut *transaction)
        .await?;

    // The reader keeps a shared lock; the second connection holds RESERVED.
    // WAL's read-to-write upgrade then returns BUSY without a native wait.
    let mut writer =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    let writer_transaction = writer.begin_with("BEGIN IMMEDIATE").await?;
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    // Freeze the admission clock before even the first attempt. No Tokio
    // timer is armed while that attempt awaits the real SQLite BUSY result.
    tokio::time::pause();
    let attempts = Cell::new(0);
    let first_busy = tokio::sync::Notify::new();
    let connection = connect_journal(options, |options| {
        attempts.set(attempts.get() + 1);
        let first_busy = &first_busy;
        async move {
            let result = SqliteConnection::connect_with(&options).await;
            if result.as_ref().err().is_some_and(|error| {
                error
                    .as_database_error()
                    .is_some_and(|error| error.code().as_deref() == Some("5"))
            }) {
                first_busy.notify_one();
            }
            result
        }
    });
    tokio::pin!(connection);
    tokio::select! {
        biased;
        () = first_busy.notified() => {}
        _ = &mut connection => return Err("connect must await a retry after the observed BUSY".into()),
    }
    let guarded = tokio::time::timeout(Duration::from_secs(3), async {
        // The connection future stays in this task, so no retry can be polled
        // between observing the first real BUSY and advancing beyond expiry.
        tokio::time::advance(Duration::from_secs(1)).await;
        let result = (&mut connection).await;
        tokio::time::resume();
        Ok::<_, Box<dyn Error>>(result)
    })
    .await;
    let result = match guarded {
        Ok(result) => result?,
        Err(error) => {
            return Err(format!(
                "retry expiry hung after {} attempts: {error}",
                attempts.get()
            )
            .into());
        }
    };

    let Err(error) = result else {
        return Err("held rollback reader must prevent the WAL-mode transition".into());
    };
    if attempts.get() != 1 {
        return Err(format!(
            "{} connections started; expiry must prevent a second attempt",
            attempts.get()
        )
        .into());
    }
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
        != Some("5")
    {
        return Err(format!("expected SQLite BUSY, got {error:?}").into());
    }
    writer_transaction.rollback().await?;
    writer.close().await?;
    transaction.rollback().await?;
    reader.close().await?;
    Ok(())
}

#[tokio::test]
async fn invalid_database_path_preserves_the_non_busy_error() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;

    let result = ObservationJournal::open(directory.path(), identity()?).await;

    let Err(JournalError::Storage(error)) = result else {
        return Err("opening a directory must return its SQLite failure".into());
    };
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
        != Some("14")
    {
        return Err(format!("expected SQLite CANTOPEN, got {error:?}").into());
    }
    Ok(())
}
