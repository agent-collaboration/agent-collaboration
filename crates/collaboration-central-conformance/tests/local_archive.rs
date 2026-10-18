#![allow(clippy::expect_used)]
use collaboration_central_conformance::{
    ArchiveCleanupRestartScenario, CleanupLoss, ConformanceClient, JwksFixtureServer,
    LeaseOrderRestartScenario, run_archive_report_conformance,
};
use collaboration_central_local::{LocalServer, LocalSettings};

#[tokio::test]
async fn automated_archive_frames_preserve_first_report_and_exact_no_ledger_finalization() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    let result = run_archive_report_conformance(&client).await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    result.expect("archive frames");
}

#[tokio::test]
async fn physical_native_registry_restart_gates_publication_and_fences_old_grants() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let settings = || LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    };
    let server = LocalServer::start(settings()).await.expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    let prepared = LeaseOrderRestartScenario::prepare(&client).await;
    server.shutdown().await.expect("shutdown");
    let prepared = prepared.expect("prepare");
    let server = LocalServer::start(settings()).await.expect("restart");
    let result = prepared
        .verify(&ConformanceClient::new(server.base_url()).expect("client"))
        .await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    result.expect("lease restart");
}

#[tokio::test]
async fn physical_native_restart_retries_release_and_acknowledgement_loss_from_persisted_alarm() {
    for loss in [CleanupLoss::Dispatch, CleanupLoss::Acknowledgement] {
        let jwks = JwksFixtureServer::start().await.expect("JWKS");
        let root = tempfile::tempdir().expect("root");
        let settings = || LocalSettings {
            root: root.path().into(),
            access_policy: collaboration_central_conformance::fixture_policy(),
            jwks_url: jwks.url.clone(),
        };
        let server = LocalServer::start(settings()).await.expect("server");
        let client = ConformanceClient::new(server.base_url()).expect("client");
        let prepared = ArchiveCleanupRestartScenario::prepare(&client, loss).await;
        server.shutdown().await.expect("shutdown");
        let prepared = prepared.expect("prepare");
        let server = LocalServer::start(settings()).await.expect("restart");
        let result = prepared
            .verify(&ConformanceClient::new(server.base_url()).expect("client"))
            .await;
        server.shutdown().await.expect("shutdown");
        jwks.shutdown().await.expect("JWKS shutdown");
        result.expect("archive cleanup restart");
    }
}
