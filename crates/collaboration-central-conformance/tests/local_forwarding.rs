#![allow(clippy::expect_used)]
use collaboration_central_conformance::*;
use collaboration_central_local::{LocalServer, LocalSettings};
#[tokio::test]
async fn forwarding_and_atomic_overload_race_use_real_clients_and_turso() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    let proof = run_forward_conformance(&client).await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    proof.expect("forward scenarios");
}
