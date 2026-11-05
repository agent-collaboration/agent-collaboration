//! Reset must never leave half a cache cleared when one statement fails.
use super::*;

#[tokio::test]
async fn a_failed_cache_clear_rolls_back_every_table_and_can_retry() {
    // Arrange real cache rows and a failure after the first table's deletion.
    let directory = tempfile::tempdir().unwrap();
    let cache = FleetCache::open(&directory.path().join("link.sqlite"))
        .await
        .unwrap();
    let key = message_board::ProjectId::generate();
    let mut connection = cache.database.pool.acquire().await.unwrap();
    sqlx::query("INSERT INTO cached_fleet_records(kind,id,payload) VALUES('project',?,'{}')")
        .bind(key.as_str())
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO fleet_cache_rebuild_records(row_id,id,kind,payload) VALUES(?,?,'project','{}')")
        .bind(key.as_str()).bind(key.as_str()).execute(&mut *connection).await.unwrap();
    sqlx::query("UPDATE fleet_cache_checkpoint SET position=9 WHERE singleton=1")
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TRIGGER fail_cache_clear BEFORE DELETE ON fleet_cache_rebuild_records BEGIN SELECT RAISE(ABORT,'injected reset failure'); END")
        .execute(&mut *connection).await.unwrap();
    drop(connection);

    // Act through the owning API, with no test replacement for its transaction.
    assert!(cache.clear_central_derived_cache().await.is_err());

    // Assert the preceding deletion and checkpoint change did not commit.
    let mut connection = cache.database.pool.acquire().await.unwrap();
    for statement in [
        "SELECT count(*) FROM cached_fleet_records",
        "SELECT count(*) FROM fleet_cache_rebuild_records",
    ] {
        let count: i64 = sqlx::query_scalar(statement)
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }
    sqlx::query("DROP TRIGGER fail_cache_clear")
        .execute(&mut *connection)
        .await
        .unwrap();
    drop(connection);
    assert_eq!(cache.checkpoint().await.unwrap().get(), 9);
    cache.clear_central_derived_cache().await.unwrap();
    assert_eq!(cache.checkpoint().await.unwrap(), FleetPosition::ZERO);
    cache.close().await;
}
