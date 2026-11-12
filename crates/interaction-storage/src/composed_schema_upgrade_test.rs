//! Actual preceding migrations: malformed schemas are refused and the backfill marker moves to its SDK home.
use crate::InteractionStore;
use sqlx::Connection;

#[tokio::test]
async fn partial_unknown_newer_or_malformed_read_context_schema_is_refused_without_rewriting() {
    for statement in [
        "DROP TABLE mail_read_contexts",
        "DELETE FROM _sqlx_migrations WHERE version=202610220001",
        "DROP INDEX mail_read_context_conversation",
        "DROP INDEX message_send_answers_settled",
        "ALTER TABLE message_send_answers ADD COLUMN unexpected TEXT",
        "ALTER TABLE mail_read_contexts ADD COLUMN unexpected TEXT",
        "UPDATE _sqlx_migrations SET checksum=x'00' WHERE version=202610220001",
        "UPDATE _sqlx_migrations SET version=202610220002 WHERE version=202610220001",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("interaction.sqlite");
        crate::schema_prefix_fixture::create_native_prefix(&path, 202610310001).await;
        let mut writer = sqlx_turso::TursoConnection::connect_with(
            &sqlx_turso::TursoConnectOptions::new().filename(&path),
        )
        .await
        .unwrap();
        sqlx::raw_sql(statement).execute(&mut writer).await.unwrap();
        writer.close().await.unwrap();
        let before = tokio::fs::read(&path).await.unwrap();
        assert!(
            matches!(
                InteractionStore::open(&path).await,
                Err(crate::StorageError::InvalidSchema)
            ),
            "accepted malformed schema: {statement}"
        );
        assert_eq!(
            tokio::fs::read(&path).await.unwrap(),
            before,
            "refusal rewrote the file: {statement}"
        );
    }
}

#[tokio::test]
async fn router_backfill_marker_moves_to_sdk_ownership_after_upgrade_and_reopen() {
    // Arrange the actual sixteen-migration predecessor, including its completed marker.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    crate::schema_prefix_fixture::create_native_prefix(&path, 202611030001).await;
    let mut connection = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new().filename(&path),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO mail_removal_backfill(id,migration_name,admitted_count,created_at,updated_at) VALUES(?,'mail-record-removals-v1',1,1,1)")
        .bind(uuid::Uuid::now_v7().to_string()).execute(&mut connection).await.unwrap();
    connection.close().await.unwrap();
    // Act through the public opener twice; no prior migration is rewritten.
    for _ in 0..2 {
        let owner = InteractionStore::open(&path).await.unwrap();
        let mut connection = owner.writer.lock().await;
        // Assert only the historical Router marker was removed, with the SDK home retained.
        let legacy: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_schema WHERE name='mail_removal_backfill'",
        )
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        let sdk: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_schema WHERE name='_fleet_sync_delivery_backfills'",
        )
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        assert_eq!((legacy, sdk), (0, 1));
        drop(connection);
        owner.close().await.unwrap();
    }
}
