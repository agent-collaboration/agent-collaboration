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

    let journal_mode = stored_journal_mode(&path).await?;
    if journal_mode != "wal" {
        return Err(
            format!("fresh interaction journal mode was {journal_mode}; expected wal").into(),
        );
    }
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
    if legacy_mode != "delete" {
        return Err(
            format!("legacy interaction journal mode was {legacy_mode}; expected delete").into(),
        );
    }

    InteractionStore::open(&path).await?.close().await?;

    let journal_mode = stored_journal_mode(&path).await?;
    if journal_mode != "wal" {
        return Err(
            format!("reopened interaction journal mode was {journal_mode}; expected wal").into(),
        );
    }
    Ok(())
}
