use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::error::Error;
use ui_preference_store::UiPreferenceStore;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "I/O setup propagates errors; assertions must fail the persistence contract test."
)]
async fn sqlite_preferences_round_trip_and_upsert_preserve_uuid_v7_identity()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("ui.sqlite");
    let store = UiPreferenceStore::open_and_migrate(&path).await;
    store.save("sessions", "group-by", "day").await?;
    store.save("quota", "group-by", "provider").await?;
    store.save("sessions", "sort", "created").await?;
    let mut raw =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    let original: String = sqlx::query_scalar("SELECT preference_id FROM ui_preferences WHERE screen = 'sessions' AND setting = 'group-by'").fetch_one(&mut raw).await?;
    assert_eq!(uuid::Uuid::parse_str(&original)?.get_version_num(), 7);
    store.save("sessions", "group-by", "machine").await?;
    let updated: String = sqlx::query_scalar("SELECT preference_id FROM ui_preferences WHERE screen = 'sessions' AND setting = 'group-by'").fetch_one(&mut raw).await?;
    assert_eq!(updated, original);
    raw.close().await?;
    store.close().await;
    let reopened = UiPreferenceStore::open_and_migrate(&path).await;
    assert_eq!(
        reopened.load("sessions", "group-by").await.as_deref(),
        Some("machine")
    );
    assert_eq!(
        reopened.load("quota", "group-by").await.as_deref(),
        Some("provider")
    );
    assert_eq!(
        reopened.load("sessions", "sort").await.as_deref(),
        Some("created")
    );
    assert_eq!(reopened.load("other", "group-by").await, None);
    reopened.close().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "I/O setup propagates errors; assertions must fail the opaque-value and schema test."
)]
async fn sqlite_preferences_keep_values_opaque_and_schema_free_of_checks()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("ui.sqlite");
    let store = UiPreferenceStore::open_and_migrate(&path).await;
    store.save("sessions", "group-by", "future-facet").await?;
    assert_eq!(
        store.load("sessions", "group-by").await.as_deref(),
        Some("future-facet")
    );
    let mut raw =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    let schema: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name = 'ui_preferences'")
            .fetch_one(&mut raw)
            .await?;
    assert!(!schema.to_ascii_uppercase().contains("CHECK"));
    assert!(
        sqlx::query("INSERT INTO ui_preferences VALUES ('duplicate', 'sessions', 'group-by', 'x')")
            .execute(&mut raw)
            .await
            .is_err()
    );
    raw.close().await?;
    store.close().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "I/O setup propagates errors; assertions must fail if damaged evidence is replaced."
)]
async fn sqlite_preferences_corrupt_file_opens_as_absent_without_replacing_evidence()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("corrupt.sqlite");
    let original = b"This is a damaged UI preferences file";
    std::fs::write(&path, original)?;
    let store = UiPreferenceStore::open_and_migrate(&path).await;
    assert_eq!(store.load("sessions", "group-by").await, None);
    assert!(store.save("sessions", "group-by", "day").await.is_err());
    store.close().await;
    assert_eq!(std::fs::read(&path)?, original);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "I/O setup propagates errors; assertions must fail if an unrelated database is adopted."
)]
async fn sqlite_preferences_unknown_database_is_not_adopted() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("other.sqlite");
    let mut raw = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await?;
    sqlx::query("CREATE TABLE unrelated (value TEXT)")
        .execute(&mut raw)
        .await?;
    raw.close().await?;
    let store = UiPreferenceStore::open_and_migrate(&path).await;
    assert_eq!(store.load("sessions", "group-by").await, None);
    assert!(store.save("sessions", "group-by", "day").await.is_err());
    store.close().await;
    let mut raw =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('ui_preferences', '_sqlx_migrations')",
    )
    .fetch_one(&mut raw)
    .await?;
    assert_eq!(count, 0);
    raw.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "I/O setup propagates errors; assertions must fail if damaged rows or writes escape the boundary."
)]
async fn sqlite_preferences_damaged_row_and_failed_write_do_not_break_reads()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("ui.sqlite");
    let store = UiPreferenceStore::open_and_migrate(&path).await;
    store.save("sessions", "group-by", "folder").await?;
    let mut raw =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    sqlx::query("UPDATE ui_preferences SET value = x'80' WHERE screen = 'sessions'")
        .execute(&mut raw)
        .await?;
    assert_eq!(store.load("sessions", "group-by").await, None);
    sqlx::query("UPDATE ui_preferences SET value = 'folder', preference_id = 'damaged' WHERE screen = 'sessions'").execute(&mut raw).await?;
    assert_eq!(store.load("sessions", "group-by").await, None);
    sqlx::query("DROP TABLE ui_preferences")
        .execute(&mut raw)
        .await?;
    assert!(store.save("sessions", "group-by", "machine").await.is_err());
    assert_eq!(store.load("sessions", "group-by").await, None);
    raw.close().await?;
    store.close().await;
    Ok(())
}
