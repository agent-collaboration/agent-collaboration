use super::FleetCache;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn fleet_login_table_reopens_in_the_existing_populated_cache() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet-cache.sqlite");
    let cache = FleetCache::open(&path).await?;
    sqlx::query("UPDATE fleet_cache_checkpoint SET position=7 WHERE singleton=1")
        .execute(&cache.database.pool).await?;
    sqlx::query("INSERT INTO fleet_login(row_id,enrollment_id,sign_in,organization_id,machine_environment,receiver_id,relay_peer_id,created_at,updated_at) VALUES ('01900000-0000-7000-8000-000000000001','01900000-0000-7000-8000-000000000002','https://sign-in.invalid','01900000-0000-7000-8000-000000000003','physical','01900000-0000-7000-8000-000000000004','01900000-0000-7000-8000-000000000005',1000,1000)")
        .execute(&cache.database.pool).await?;
    cache.close().await;
    let reopened = FleetCache::open(&path).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fleet_login")
        .fetch_one(&reopened.database.pool).await?;
    let checkpoint: i64 = sqlx::query_scalar("SELECT position FROM fleet_cache_checkpoint WHERE singleton=1")
        .fetch_one(&reopened.database.pool).await?;
    if count != 1 || checkpoint != 7 {
        return Err("login or existing cache row did not survive reopen".into());
    }
    reopened.close().await;
    Ok(())
}
