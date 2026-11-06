use super::{JOURNAL_BUSY_TIMEOUT, JournalError, ObservationJournal};
use collaboration_protocol::UuidIdentity;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::error::Error;

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

    if u128::try_from(busy_timeout_ms)? != JOURNAL_BUSY_TIMEOUT.as_millis() {
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

    let result = tokio::time::timeout(
        JOURNAL_BUSY_TIMEOUT * 3,
        ObservationJournal::open(&path, identity()?),
    )
    .await?;

    let Err(JournalError::Storage(error)) = result else {
        return Err("held rollback reader must prevent the WAL-mode transition".into());
    };
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
        != Some("5")
    {
        return Err(format!("expected SQLite BUSY, got {error:?}").into());
    }
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
