use super::ProjectStore;
use message_board::ProjectId;
use sqlx::{ConnectOptions, Connection};

#[tokio::test]
async fn holder_bookkeeping_uses_an_independent_uncaptured_migration_track()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut application_only = sqlx_turso::TursoConnectOptions::new().connect().await?;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut application_only)
        .await?;
    let mut application_transaction = application_only.begin_with("BEGIN IMMEDIATE").await?;
    crate::PROJECT_MIGRATOR
        .run_direct(None, &mut *application_transaction, false)
        .await?;
    application_transaction.commit().await?;
    let local_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_schema WHERE lower(name) GLOB '_fleet_sync_*'",
    )
    .fetch_one(&mut application_only)
    .await?;
    assert_eq!(
        local_tables, 0,
        "replicated migrations must create no holder bookkeeping"
    );
    application_only.close().await?;
    let directory = tempfile::tempdir()?;
    let store = ProjectStore::open(
        &directory.path().join("project.sqlite"),
        ProjectId::generate(),
    )
    .await?;
    let mut read = store.database.reader().begin_read().await?;
    let application_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&mut read)
        .await?;
    let local_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _fleet_sync_local_migrations")
        .fetch_one(&mut read)
        .await?;
    assert_eq!(
        application_count,
        i64::try_from(crate::PROJECT_MIGRATOR.iter().count())?
    );
    assert_eq!(local_count, 1);
    let captured_schema: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM turso_cdc WHERE table_name = 'sqlite_schema' AND instr(after, '_fleet_sync_') > 0").fetch_one(&mut read).await?;
    let captured_history: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM turso_cdc WHERE table_name = '_fleet_sync_local_migrations'",
    )
    .fetch_one(&mut read)
    .await?;
    let captured_application_history: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM turso_cdc WHERE table_name = '_sqlx_migrations'")
            .fetch_one(&mut read)
            .await?;
    assert!(
        captured_application_history >= application_count,
        "authoritative application migrations must remain captured"
    );
    assert_eq!(
        captured_schema, 0,
        "local DDL must not enter the replicated stream"
    );
    assert_eq!(
        captured_history, 0,
        "local migration history must not enter the stream"
    );
    read.close().await?;
    store.close().await;
    Ok(())
}
