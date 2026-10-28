#![cfg(feature = "conformance")]
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, fixture_policy};
use collaboration_central_tokio::{TokioServer, TokioSettings};
#[path = "native_http/support.rs"]
mod configuration;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/copy_wait_scenarios.rs"]
mod copy_wait_scenarios;
#[path = "support/copy_read_scenarios.rs"]
mod scenarios;

#[tokio::test]
async fn snapshot_reads_keep_a_session_usable_after_another_copy_is_unavailable_or_retired()
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
    let proof = scenarios::run(server.base_url()).await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    proof
}
