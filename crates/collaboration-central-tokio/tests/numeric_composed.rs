//! The shared scalar oracle survives loss of every native runtime object.
#![cfg(feature = "conformance")]

#[path = "native_http/support.rs"]
mod configuration;
use collaboration_central_conformance::{
    ComposedNumericScenario, ConformanceClient, ConformanceResult, JwksFixtureServer,
    fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::time::Duration;

#[tokio::test]
async fn exact_scalar_cells_and_receipts_survive_whole_native_server_reopen()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let settings = || {
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(TokioSettings {
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
    };
    let first = TokioServer::start(settings()?).await?;
    let prepared =
        ComposedNumericScenario::prepare(&ConformanceClient::new(first.base_url())?).await;
    first.shutdown().await?;
    let proof = async {
        let prepared = prepared?;
        let second = TokioServer::start(settings()?).await?;
        let proof = prepared
            .verify(&ConformanceClient::new(second.base_url())?)
            .await;
        second.shutdown().await?;
        proof
    }
    .await;
    jwks.shutdown().await?;
    proof
}
