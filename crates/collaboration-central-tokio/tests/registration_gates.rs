//! Creation gates run through normal HTTP enrollment and machine sessions.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, fixture_policy};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::time::Duration;

#[tokio::test]
async fn real_registry_and_fleet_apply_all_four_creation_gate_outcomes() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start_conformance(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof = creation_scenarios::run(server.base_url()).await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
