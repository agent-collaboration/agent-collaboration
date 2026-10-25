#![cfg(feature = "conformance")]
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, fixture_policy,
    run_revocation_race_conformance,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
#[path = "native_http/support.rs"]
mod configuration;

#[tokio::test]
async fn revocation_gates_readmission_and_a_late_duplicate_cannot_end_its_new_session()
-> ConformanceResult<()> {
    let jwks = JwksFixtureServer::start().await?;
    let root = tempfile::tempdir()?;
    let server = TokioServer::start(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let result = run_revocation_race_conformance(&ConformanceClient::new(server.base_url())?).await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}

#[tokio::test]
async fn a_saved_activation_cannot_undo_revocation_while_enrollment_finish_was_paused()
-> ConformanceResult<()> {
    let jwks = JwksFixtureServer::start().await?;
    let root = tempfile::tempdir()?;
    let server = TokioServer::start(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let result = collaboration_central_conformance::run_readmission_activation_conformance(
        &ConformanceClient::new(server.base_url())?,
    )
    .await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}

#[tokio::test]
async fn two_real_readmissions_hold_open_and_keep_overlapping_credentials_inactive()
-> ConformanceResult<()> {
    let jwks = JwksFixtureServer::start().await?;
    let root = tempfile::tempdir()?;
    let server = TokioServer::start(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let result = collaboration_central_conformance::run_readmission_open_conformance(
        &ConformanceClient::new(server.base_url())?,
    )
    .await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}
