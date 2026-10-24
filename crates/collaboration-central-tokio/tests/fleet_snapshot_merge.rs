//! The same ordered snapshot scenario also runs against real workerd.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, fixture_policy,
    run_fleet_snapshot_merge_conformance,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::time::Duration;

#[tokio::test]
async fn snapshot_merge_preserves_kind_ties_and_the_public_page_boundary() -> ConformanceResult<()>
{
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
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
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof =
        run_fleet_snapshot_merge_conformance(&ConformanceClient::new(server.base_url())?).await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
