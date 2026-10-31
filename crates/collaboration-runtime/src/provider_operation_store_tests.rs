use std::error::Error;

use sqlx::{
    Connection, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};

use super::ProviderOperationStore;

#[tokio::test]
async fn new_provider_operation_database_uses_incremental_auto_vacuum() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("provider-operations.sqlite");
    let mut store = ProviderOperationStore::open(&database_path).await?;

    let mode: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut store.connection)
        .await?;
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut store.connection)
        .await?;
    assert_eq!(mode, 2);
    assert_eq!(journal_mode, "wal");
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn existing_delete_mode_provider_database_reopens_in_wal_mode() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("provider-operations.sqlite");
    ProviderOperationStore::open(&database_path)
        .await?
        .close()
        .await?;

    let mut legacy = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&database_path)
            .journal_mode(SqliteJournalMode::Delete),
    )
    .await?;
    let legacy_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut legacy)
        .await?;
    legacy.close().await?;
    assert_eq!(legacy_mode, "delete");

    let mut store = ProviderOperationStore::open(&database_path).await?;
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut store.connection)
        .await?;
    assert_eq!(journal_mode, "wal");
    store.close().await?;
    Ok(())
}
