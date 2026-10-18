#![allow(clippy::expect_used)]
use collaboration_central_conformance::{
    ConformanceClient, JwksFixtureServer, NoticeRestartScenario, run_fleet_notice_conformance,
};
use collaboration_central_conformance::{FleetOutboxRestartScenario, NoticeDeliveryLoss};
use collaboration_central_local::{LocalServer, LocalSettings};

#[tokio::test]
async fn registry_notice_and_seven_day_alarm_survive_physical_native_restart() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let settings = || LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    };
    let server = LocalServer::start(settings()).await.expect("server");
    let prepared =
        NoticeRestartScenario::prepare(&ConformanceClient::new(server.base_url()).expect("client"))
            .await;
    server.shutdown().await.expect("shutdown");
    let prepared = prepared.expect("prepare");
    let server = LocalServer::start(settings()).await.expect("restart");
    let result = prepared
        .verify(&ConformanceClient::new(server.base_url()).expect("client"))
        .await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    result.expect("notice restart");
}

#[tokio::test]
async fn fleet_alarm_delivers_numbered_hints_through_real_native_sockets() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let result =
        run_fleet_notice_conformance(&ConformanceClient::new(server.base_url()).expect("client"))
            .await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    result.expect("Fleet notices");
}

#[tokio::test]
async fn fleet_outbox_restarts_after_dispatch_or_real_acknowledgement_loss() {
    for loss in [
        NoticeDeliveryLoss::Dispatch,
        NoticeDeliveryLoss::Acknowledgement,
    ] {
        let jwks = JwksFixtureServer::start().await.expect("JWKS");
        let root = tempfile::tempdir().expect("root");
        let settings = || LocalSettings {
            root: root.path().into(),
            access_policy: collaboration_central_conformance::fixture_policy(),
            jwks_url: jwks.url.clone(),
        };
        let server = LocalServer::start(settings()).await.expect("server");
        let prepared = FleetOutboxRestartScenario::prepare(
            &ConformanceClient::new(server.base_url()).expect("client"),
            loss,
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
        result.expect("Fleet outbox restart");
    }
}
