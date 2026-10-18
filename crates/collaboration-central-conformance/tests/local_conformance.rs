#![allow(clippy::expect_used)]
use collaboration_central_conformance::{
    ConformanceClient, run_copy_conformance, run_session_conformance,
};
use collaboration_central_local::{LocalServer, LocalSettings};
#[tokio::test]
async fn shared_copy_suite_on_real_turso() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS server");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    run_copy_conformance(&client).await.expect("copy scenarios");
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
}
#[tokio::test]
async fn shared_session_suite_on_real_websockets_and_turso() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS server");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    run_session_conformance(&client)
        .await
        .expect("session scenarios");
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
}

#[tokio::test]
async fn session_recovers_lost_ack_after_entire_local_server_is_reopened() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS server");
    use collaboration_central_conformance::SessionResumeScenario;
    let root = tempfile::tempdir().expect("root");
    let settings = || LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    };
    let first = LocalServer::start(settings()).await.expect("first server");
    let client = ConformanceClient::new(first.base_url()).expect("client");
    let scenario = SessionResumeScenario::prepare(&client)
        .await
        .expect("lost ack setup");
    first
        .shutdown()
        .await
        .expect("drop all runtime and actor memory");
    let second = LocalServer::start(settings())
        .await
        .expect("reopen same files");
    let client = ConformanceClient::new(second.base_url()).expect("new client");
    scenario.verify(&client).await.expect("persisted resume");
    second.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
}

#[tokio::test]
async fn transaction_rollbacks_and_complete_concurrent_reads_survive_local_server_reopen() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS server");
    use collaboration_central_conformance::TransactionRestartScenario;
    let root = tempfile::tempdir().expect("root");
    let settings = || LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    };
    let first = LocalServer::start(settings()).await.expect("first server");
    let client = ConformanceClient::new(first.base_url()).expect("client");
    let scenario = TransactionRestartScenario::prepare(&client)
        .await
        .expect("rollbacks, fence and concurrent read");
    first
        .shutdown()
        .await
        .expect("drop all runtime and actor memory");
    let second = LocalServer::start(settings())
        .await
        .expect("reopen same files");
    let client = ConformanceClient::new(second.base_url()).expect("new client");
    scenario
        .verify(&client)
        .await
        .expect("persisted rows and positions");
    second.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
}

#[tokio::test]
async fn lossless_sql_scalars_survive_real_storage_and_snapshot() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS server");
    use collaboration_central_conformance::run_numeric_conformance;
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    run_numeric_conformance(&client)
        .await
        .expect("full i64 keys/values and REAL 1.0 remain distinct");
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
}

#[tokio::test]
async fn staging_safe_session_groups_pass_on_real_websockets_and_turso() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS server");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    collaboration_central_conformance::run_staging_session_conformance(&client)
        .await
        .expect("staging-safe session groups");
    collaboration_central_conformance::run_staging_replacement_row(&client)
        .await
        .expect("S4 replacement with a held renewal");
    collaboration_central_conformance::run_staging_revocation_row(&client)
        .await
        .expect("A10 revocation of a live session");
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
}

#[tokio::test]
async fn staging_normal_rows_pass_across_local_server_reopen() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS server");
    let root = tempfile::tempdir().expect("root");
    let settings = || LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    };
    let first = LocalServer::start(settings()).await.expect("first server");
    let client = ConformanceClient::new(first.base_url()).expect("client");
    collaboration_central_conformance::run_staging_transport_rows(&client)
        .await
        .expect("normal clean move/forward and unread owner response");
    collaboration_central_conformance::run_staging_renewal_rows(&client)
        .await
        .expect("renewal replay through a takeover");
    collaboration_central_conformance::run_staging_hibernation_rows(
        &client,
        &[std::time::Duration::ZERO],
    )
    .await
    .expect("same-socket frame after a window");
    let mut state = collaboration_central_conformance::prepare_staging_normal_rows(&client)
        .await
        .expect("fence, takeover and lost-answer rows");
    first.shutdown().await.expect("stand-in for the redeploy");
    assert!(
        state
            .await_deploy_close(std::time::Duration::from_secs(10))
            .await
            .expect("socket close"),
        "the server going away closes the open session"
    );
    let second = LocalServer::start(settings())
        .await
        .expect("reopen same files");
    let client = ConformanceClient::new(second.base_url()).expect("new client");
    state.verify(&client).await.expect("rows after the reopen");
    second.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
}

#[tokio::test]
async fn held_fence_and_held_piece_resolve_to_one_order() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS server");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    collaboration_central_conformance::held_fence_and_piece_resolve_to_one_complete_prefix(&client)
        .await
        .expect("one order, one complete prefix");
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
}

#[tokio::test]
async fn surviving_normal_socket_is_read_before_the_honest_reconnect() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    let mut state = collaboration_central_conformance::prepare_staging_normal_rows(&client)
        .await
        .expect("normal state");
    assert!(
        !state
            .await_deploy_close(std::time::Duration::ZERO)
            .await
            .expect("survival observation")
    );
    state
        .verify(&client)
        .await
        .expect("same socket exact read then reconnect");
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
}
