//! Normal owner operations share their runtime scenario with the Worker shell.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/owner_registry_scenarios.rs"]
mod owner_registry_scenarios;
#[tokio::test]
async fn normal_fleet_creation_owner_move_takeover_and_migrate()
-> collaboration_central_conformance::ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let keys = collaboration_central_conformance::JwksFixtureServer::start().await?;
    let server = collaboration_central_tokio::TokioServer::start(
        collaboration_central_tokio::TokioSettings {
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: fleet_sync_protocol::PeerId::try_from(
                    "019f0000-0000-7000-8000-000000005240".to_owned(),
                )?,
            },
            root: root.path().into(),
            access_policy: collaboration_central_conformance::fixture_policy(),
            jwks_url: keys.url.clone(),
            registry: configuration::registry_config()?,
            holder_answer_timeout: std::time::Duration::from_secs(2),
            copy_ack_wait: std::time::Duration::from_secs(2),
        },
    )
    .await?;
    let proof = owner_registry_scenarios::run(server.base_url()).await;
    let stopped = server.shutdown().await;
    let keys_stopped = keys.shutdown().await;
    proof?;
    stopped?;
    keys_stopped?;
    Ok(())
}
