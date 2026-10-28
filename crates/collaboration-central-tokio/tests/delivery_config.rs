//! A non-fixture relay setting crosses the real native Registry boundary.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;
#[path = "support/delivery_config_scenarios.rs"]
mod delivery_config_scenarios;
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, fixture_policy};
use collaboration_central_tokio::{DeliveryConfig, TokioServer, TokioSettings};
use fleet_sync_protocol::PeerId;
use std::time::Duration;

#[tokio::test]
async fn configured_relay_survives_enrollment_replay_update_and_real_readmission()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    // Deliberately differ from both the receiver and the shared fixture value.
    let relay_peer = PeerId::try_from("019f0000-0000-7000-8000-000000005241".to_owned())?;
    let server = TokioServer::start(TokioSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        delivery: DeliveryConfig { relay_peer },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof = delivery_config_scenarios::run(server.base_url(), relay_peer).await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
