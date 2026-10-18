#![allow(clippy::expect_used)]
use collaboration_central_conformance::{
    ConformanceClient, FleetFeedRestartScenario, JwksFixtureServer,
};
use collaboration_central_local::{LocalServer, LocalSettings};

#[tokio::test]
async fn fleet_feed_survives_physical_restart_and_prunes_by_central_append_time() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let settings = || LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    };
    let server = LocalServer::start(settings()).await.expect("server");
    let prepared = FleetFeedRestartScenario::prepare(
        &ConformanceClient::new(server.base_url()).expect("client"),
    )
    .await;
    server.shutdown().await.expect("shutdown");
    let prepared = prepared.expect("prepare");
    let server = LocalServer::start(settings()).await.expect("restart");
    let pruned = prepared
        .until_pruned(&ConformanceClient::new(server.base_url()).expect("client"))
        .await;
    server.shutdown().await.expect("shutdown");
    let pruned = pruned.expect("retention phase");
    // The adapter's address_key maps Fleet to this exact task-owned file.
    // Read it only after all server/actor/storage writers have shut down.
    let observed=tokio::process::Command::new("python3").arg("-c").arg(
        "import json,pathlib,sqlite3,sys\np=pathlib.Path(sys.argv[1]).resolve()\nc=sqlite3.connect(p.as_uri()+'?mode=ro',uri=True)\nprint(json.dumps([c.execute('SELECT count(*) FROM fleet_feed').fetchone()[0],c.execute('SELECT last_position FROM fleet_checkpoint WHERE singleton=1').fetchone()[0],c.execute('SELECT through_position FROM fleet_feed_retention_floor WHERE singleton=1').fetchone()[0]]))\nc.close()"
    ).arg(root.path().join("fleet.sqlite")).output().await.expect("read-only actual SQLite observation");
    assert!(
        observed.status.success(),
        "stopped Fleet read failed: {}",
        String::from_utf8_lossy(&observed.stderr)
    );
    let values: [u64; 3] =
        serde_json::from_slice(&observed.stdout).expect("actual count/head/floor");
    assert_eq!(values, [0, pruned.retained_head(), pruned.retained_head()]);
    println!(
        "FLEET FEED stopped actual SQLite: rows={}, checkpoint={}, retention_floor={}",
        values[0], values[1], values[2]
    );
    let server = LocalServer::start(settings())
        .await
        .expect("restart after observed prune-all");
    let result = pruned
        .append(&ConformanceClient::new(server.base_url()).expect("client"))
        .await;
    server.shutdown().await.expect("post-prune shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    result.expect("Fleet feed restart");
}
