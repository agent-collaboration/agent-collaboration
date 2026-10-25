//! Open the original SQLite history file in place, with its actual migration lineage.
use super::*;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

static LEGACY_MIGRATOR: sqlx::migrate::Migrator =
    sqlx::migrate!("./tests/fixtures/legacy-history-migrations");

#[tokio::test]
async fn original_sqlite_history_opens_in_place_with_unchanged_rows_and_migration_checksum() {
    // Arrange an original-engine file before the native owner has opened it.
    let directory = tempfile::tempdir().expect("legacy history fixture");
    let path = directory.path().join("interaction.sqlite");
    let mut legacy = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .expect("original SQLite owner");
    LEGACY_MIGRATOR
        .run(&mut legacy)
        .await
        .expect("original migration");
    let original_record = refused_approval("opaque-before-turso");
    let original_json = serde_json::to_string(&original_record).expect("original record");
    let original_stamp = "2026-10-18T01:02:03.000000007Z";
    sqlx::query(
        "INSERT INTO typed_interaction_history(request_id,record_json,created_at) VALUES(?,?,?)",
    )
    .bind(original_record.request_id())
    .bind(&original_json)
    .bind(original_stamp)
    .execute(&mut legacy)
    .await
    .expect("original history row");
    sqlx::query("INSERT INTO interaction_history_revision(metadata_id,revision) VALUES(1,7)")
        .execute(&mut legacy)
        .await
        .expect("original revision");
    let original_checksum: Vec<u8> =
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version=202610080001")
            .fetch_one(&mut legacy)
            .await
            .expect("original migration checksum");
    legacy.close().await.expect("close original engine");
    #[cfg(unix)]
    let original_identity = {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(&path).expect("original file identity");
        (metadata.dev(), metadata.ino())
    };

    // Act through the new native owner, including a real native mutation.
    let history = InteractionHistoryStore::load(path.clone())
        .await
        .expect("native in-place open");
    assert_eq!(history.list_all().await, vec![original_record]);
    assert_eq!(history.data.lock().await.revision, 7);
    sqlite_scenarios::add_refusal(&history, "native-after-upgrade")
        .await
        .expect("native history mutation");

    // Assert durable old bytes and lineage alongside the new native row.
    let mut connection = history.owner.writer.lock().await;
    let retained: (String, String) = sqlx::query_as("SELECT record_json,created_at FROM typed_interaction_history WHERE request_id='opaque-before-turso'")
        .fetch_one(&mut *connection).await.expect("retained original row");
    assert_eq!(retained, (original_json, original_stamp.to_owned()));
    let checksum: Vec<u8> =
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version=202610080001")
            .fetch_one(&mut *connection)
            .await
            .expect("retained original checksum");
    assert_eq!(checksum, original_checksum);
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM interaction_history_revision")
        .fetch_one(&mut *connection)
        .await
        .expect("native committed revision");
    assert_eq!(revision, 8);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM typed_interaction_history")
        .fetch_one(&mut *connection)
        .await
        .expect("combined history");
    assert_eq!(count, 2);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).expect("native file identity");
        assert_eq!((metadata.dev(), metadata.ino()), original_identity);
    }
}
