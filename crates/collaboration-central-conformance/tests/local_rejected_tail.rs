#![allow(clippy::expect_used)]
use collaboration_central_conformance::{
    ConformanceClient, JwksFixtureServer, RejectedTailRestartScenario,
};
use collaboration_central_local::{LocalServer, LocalSettings};

#[tokio::test]
async fn verified_tail_survives_physical_native_restart_and_is_readable_by_successor() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let settings = || LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    };
    let server = LocalServer::start(settings()).await.expect("server");
    let prepared = RejectedTailRestartScenario::prepare(
        &ConformanceClient::new(server.base_url()).expect("client"),
    )
    .await;
    server.shutdown().await.expect("shutdown");
    let prepared = prepared.expect("prepare");
    let server = LocalServer::start(settings()).await.expect("restart");
    let result = prepared
        .verify(&ConformanceClient::new(server.base_url()).expect("client"))
        .await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    result.expect("tail restart");
}
