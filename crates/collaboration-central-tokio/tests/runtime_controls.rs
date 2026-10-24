//! Every runtime control reaches the owning object through actual native traffic.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, fixture_policy,
    run_runtime_control_conformance,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};

#[tokio::test]
async fn controls_hit_real_composed_objects_without_replacing_their_transactions_or_sockets()
-> ConformanceResult<()> {
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
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let proof = run_runtime_control_conformance(&ConformanceClient::new(server.base_url())?).await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
