//! The native adapter runs the shared normal-session directory contract.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;
#[path = "support/installation_peer_directory_scenarios.rs"]
mod scenarios;
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, fixture_policy};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::time::Duration;

#[tokio::test]
async fn pages_keep_native_order_and_revoked_bindings_while_submit_still_refuses()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let keys = JwksFixtureServer::start().await?;
    let server = TokioServer::start(TokioSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: keys.url.clone(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof = scenarios::run(server.base_url()).await;
    let stopped = server.shutdown().await;
    let keys_closed = keys.shutdown().await;
    proof?;
    stopped?;
    keys_closed?;
    Ok(())
}
