#![cfg(feature = "conformance")]

#[path = "native_http/support.rs"]
mod configuration;
#[path = "support/copy_wait_scenarios.rs"]
mod scenarios;
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, fixture_policy};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::time::Duration;

#[tokio::test]
async fn copy_waits_follow_the_real_status_commit_signals_and_existing_alarm()
-> ConformanceResult<()> {
    let jwks = JwksFixtureServer::start().await?;
    let directory = tempfile::tempdir()?;
    let server = TokioServer::start_conformance(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: directory.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let result = scenarios::run(server.base_url()).await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}
