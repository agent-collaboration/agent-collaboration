use std::error::Error;

use interaction_storage::InteractionStore;
use sqlx::{
    Connection, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use sqlx_turso::{TursoConnectOptions, TursoConnection};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

async fn stored_journal_mode(path: &std::path::Path) -> Result<String, sqlx::Error> {
    let options = TursoConnectOptions::new().filename(path).read_only(true);
    let mut connection = TursoConnection::connect_with(&options).await?;
    let journal_mode = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut connection)
        .await?;
    connection.close().await?;
    Ok(journal_mode)
}

#[tokio::test]
async fn fresh_interaction_database_uses_wal() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("interaction.sqlite");
    InteractionStore::open(&path).await?.close().await?;

    assert_eq!(stored_journal_mode(&path).await?, "wal");
    Ok(())
}

#[tokio::test]
async fn existing_delete_mode_interaction_database_reopens_in_wal() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("interaction.sqlite");
    InteractionStore::open(&path).await?.close().await?;

    let mut legacy = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .journal_mode(SqliteJournalMode::Delete),
    )
    .await?;
    let legacy_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut legacy)
        .await?;
    legacy.close().await?;
    assert_eq!(legacy_mode, "delete");

    InteractionStore::open(&path).await?.close().await?;

    assert_eq!(stored_journal_mode(&path).await?, "wal");
    Ok(())
}
