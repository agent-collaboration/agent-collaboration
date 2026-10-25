use super::*;
use fixture::{
    Live, Reader, age_feed, assert_floor, assert_staging_rows, assert_visible, create, update,
};
use ingress::ReadPoint;
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    executor::Executor,
};

async fn seed_old_view(server_root: &Path, router_root: &Path, jwks: &str) -> TestResult {
    let mut live = Live::start(server_root, jwks).await?;
    live.commit(create()?).await?;
    let reader = Reader::start(router_root, live.server.base_url()).await?;
    reader.fleet.synchronize().await?;
    assert_visible(&reader.fleet, router_root, "old visible workspace", 2).await?;
    // Advance central beyond the saved cache before retention is aged.
    live.commit(update("pruned while Router was behind")?)
        .await?;
    reader.shutdown().await?;
    live.shutdown().await?;
    age_feed(server_root).await
}
async fn large_snapshot(live: &mut Live) -> TestResult {
    for index in 0..254 {
        live.commit(update(&format!("snapshot setup {index}"))?)
            .await?;
    }
    Ok(())
}

#[tokio::test]
async fn router_rebuild_keeps_the_old_view_until_older_then_newer_catchup_finishes() -> TestResult {
    // Wrong behavior: publishing snapshot or older replay pages before catch-up.
    // Expectations are literal inputs/positions; the store, client, pump and
    // core are real. The Access edge holds requests without fabricating replies.
    let directory = tempfile::tempdir()?;
    let server_root = directory.path().join("central");
    let router_root = directory.path().join("router");
    let jwks = JwksFixtureServer::start().await?;
    seed_old_view(&server_root, &router_root, &jwks.url).await?;
    let mut live = Live::start(&server_root, &jwks.url).await?;
    large_snapshot(&mut live).await?; // head512, retained floor4, saved cache2.
    assert_floor(&server_root, 4).await?;
    let reader = Reader::start(&router_root, live.server.base_url()).await?;
    let mut snapshot = reader
        .ingress
        .controls
        .hold(ReadPoint::SnapshotContinuation)
        .await;
    let mut catchup = reader
        .ingress
        .controls
        .hold(ReadPoint::FeedAfter(768))
        .await;
    let fleet = reader.fleet.clone();
    let refresh = tokio::spawn(async move { fleet.synchronize().await });
    snapshot.entered().await?;
    assert_staging_rows(&router_root, 256).await?;
    assert_visible(&reader.fleet, &router_root, "old visible workspace", 2).await?;
    // The final snapshot page sees this newest current row at771, although the
    // frozen cut is512. Its feed first replays genuinely older versions.
    for index in 0..130 {
        live.commit(update(&format!("catch-up state {index}"))?)
            .await?;
    }
    snapshot.release()?;
    catchup.entered().await?;
    assert_visible(&reader.fleet, &router_root, "old visible workspace", 2).await?;
    catchup.release()?;
    tokio::time::timeout(BOUND, refresh).await???;
    assert_visible(&reader.fleet, &router_root, "catch-up state 129", 772).await?;
    assert!(
        !router_root.join("fleet.sqlite").exists(),
        "Router recreated a local fleet authority"
    );
    reader.shutdown().await?;
    live.shutdown().await?;
    jwks.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn router_restart_discards_a_partial_snapshot_after_its_cut_expires() -> TestResult {
    // A partial build survives on disk; after restart its expired cut must not
    // become an active projection or a resumed server-side snapshot registry.
    let directory = tempfile::tempdir()?;
    let server_root = directory.path().join("central");
    let router_root = directory.path().join("router");
    let jwks = JwksFixtureServer::start().await?;
    seed_old_view(&server_root, &router_root, &jwks.url).await?;
    let mut live = Live::start(&server_root, &jwks.url).await?;
    large_snapshot(&mut live).await?;
    let reader = Reader::start(&router_root, live.server.base_url()).await?;
    let mut snapshot = reader
        .ingress
        .controls
        .hold(ReadPoint::SnapshotContinuation)
        .await;
    let fleet = reader.fleet.clone();
    let refresh = tokio::spawn(async move { fleet.synchronize().await });
    snapshot.entered().await?;
    assert_staging_rows(&router_root, 256).await?;
    assert_visible(&reader.fleet, &router_root, "old visible workspace", 2).await?;
    live.shutdown().await?;
    snapshot.release()?;
    assert!(tokio::time::timeout(BOUND, refresh).await??.is_err());
    reader.shutdown().await?;
    age_feed(&server_root).await?;
    let mut live = Live::start(&server_root, &jwks.url).await?;
    live.commit(update("fresh after expired snapshot cut")?)
        .await?; // floor512.
    assert_floor(&server_root, 512).await?;
    let reader = Reader::start(&router_root, live.server.base_url()).await?;
    assert_visible(&reader.fleet, &router_root, "old visible workspace", 2).await?;
    reader.fleet.synchronize().await?;
    assert_visible(
        &reader.fleet,
        &router_root,
        "fresh after expired snapshot cut",
        514,
    )
    .await?;
    assert_staging_rows(&router_root, 0).await?;
    reader.shutdown().await?;
    live.shutdown().await?;
    jwks.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn router_reopen_replaces_an_expired_view_from_a_fresh_snapshot() -> TestResult {
    // A consumer that treats CursorExpired as caught up leaves the exact old
    // view here. No frame hold or publication fault can mask that error.
    let directory = tempfile::tempdir()?;
    let server_root = directory.path().join("central");
    let router_root = directory.path().join("router");
    let jwks = JwksFixtureServer::start().await?;
    seed_old_view(&server_root, &router_root, &jwks.url).await?;
    let mut live = Live::start(&server_root, &jwks.url).await?;
    live.commit(update("fresh snapshot after retention")?)
        .await?;
    assert_floor(&server_root, 4).await?;
    let reader = Reader::start(&router_root, live.server.base_url()).await?;
    assert_visible(&reader.fleet, &router_root, "old visible workspace", 2).await?;
    reader.fleet.synchronize().await?;
    assert_visible(
        &reader.fleet,
        &router_root,
        "fresh snapshot after retention",
        6,
    )
    .await?;
    reader.shutdown().await?;
    live.shutdown().await?;
    jwks.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn router_publication_failure_preserves_active_records_and_checkpoint_until_retry()
-> TestResult {
    // An actual native final INSERT fails; active rows/checkpoint must roll back.
    let directory = tempfile::tempdir()?;
    let server_root = directory.path().join("central");
    let router_root = directory.path().join("router");
    let jwks = JwksFixtureServer::start().await?;
    seed_old_view(&server_root, &router_root, &jwks.url).await?;
    let mut cache = sqlx_turso::TursoConnectOptions::new()
        .filename(router_root.join("fleet-cache.sqlite"))
        .connect()
        .await?;
    cache.execute("CREATE TRIGGER reject_cache_publication BEFORE INSERT ON cached_fleet_records BEGIN SELECT RAISE(ABORT,'fixture publication failure'); END").await?;
    cache.close().await?;
    let mut live = Live::start(&server_root, &jwks.url).await?;
    live.commit(update("published only after retry")?).await?;
    let reader = Reader::start(&router_root, live.server.base_url()).await?;
    assert!(reader.fleet.synchronize().await.is_err());
    assert_visible(&reader.fleet, &router_root, "old visible workspace", 2).await?;
    reader.shutdown().await?;
    let mut cache = sqlx_turso::TursoConnectOptions::new()
        .filename(router_root.join("fleet-cache.sqlite"))
        .connect()
        .await?;
    cache
        .execute("DROP TRIGGER reject_cache_publication")
        .await?;
    cache.close().await?;
    let reader = Reader::start(&router_root, live.server.base_url()).await?;
    reader.fleet.synchronize().await?;
    assert_visible(&reader.fleet, &router_root, "published only after retry", 6).await?;
    reader.shutdown().await?;
    live.shutdown().await?;
    jwks.shutdown().await?;
    Ok(())
}
