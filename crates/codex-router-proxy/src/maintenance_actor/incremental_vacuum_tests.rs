use std::error::Error;

use codex_router_state::sqlite::AsyncSqliteStateStore;

use super::{MaintenanceHint, MaintenanceRepository};

#[tokio::test]
async fn state_retention_hint_releases_one_bounded_page_batch() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let database_path = directory.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path).await?;
    let (page_count_before, freelist_before) = {
        let mut connection = store.acquire_connection_for_test().await?;
        sqlx::query("CREATE TABLE vacuum_fixture(payload BLOB NOT NULL)")
            .execute(&mut *connection)
            .await?;
        sqlx::query(
            "WITH RECURSIVE row_number(value) AS (
                 VALUES (1)
                 UNION ALL
                 SELECT value + 1 FROM row_number WHERE value < 600
             )
             INSERT INTO vacuum_fixture(payload)
             SELECT zeroblob(4096) FROM row_number",
        )
        .execute(&mut *connection)
        .await?;
        sqlx::query("DELETE FROM vacuum_fixture")
            .execute(&mut *connection)
            .await?;
        let page_count: i64 = sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(&mut *connection)
            .await?;
        let freelist_count: i64 = sqlx::query_scalar("PRAGMA freelist_count")
            .fetch_one(&mut *connection)
            .await?;
        (page_count, freelist_count)
    };
    assert!(freelist_before > 64);

    store
        .run_maintenance_hint(MaintenanceHint::CleanupStaleSessionAccountAffinities {
            stale_before_unix_seconds: 0,
        })
        .await?;

    let mut connection = store.acquire_connection_for_test().await?;
    let page_count_after: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(&mut *connection)
        .await?;
    let pages_released = page_count_before - page_count_after;
    assert!(pages_released > 0);
    assert!(pages_released <= 64);
    drop(connection);
    store.close().await?;
    Ok(())
}
