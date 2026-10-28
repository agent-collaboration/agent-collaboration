#![cfg(feature = "conformance")]

use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, fixture_policy,
    run_session_replacement_conformance,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::time::Duration;
#[path = "native_http/support.rs"]
mod configuration;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/copy_read_scenarios.rs"]
mod copy_read_scenarios;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/copy_wait_scenarios.rs"]
mod copy_wait_scenarios;
#[path = "support/malformed_frame_scenarios.rs"]
mod malformed_frame_scenarios;

enum SessionCase {
    Replacement,
    MalformedFrames,
}

#[tokio::test]
async fn authenticated_upgrade_welcome_and_replacement_run_through_real_websockets()
-> ConformanceResult<()> {
    run_case(SessionCase::Replacement).await
}

#[tokio::test]
async fn malformed_frames_get_correlated_refusals_or_typed_physical_close() -> ConformanceResult<()>
{
    run_case(SessionCase::MalformedFrames).await
}

async fn run_case(case: SessionCase) -> ConformanceResult<()> {
    let jwks = JwksFixtureServer::start().await?;
    let directory = tempfile::tempdir()?;
    let server = TokioServer::start(TokioSettings {
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
    let result = match case {
        SessionCase::Replacement => {
            run_session_replacement_conformance(&ConformanceClient::new(server.base_url())?).await
        }
        SessionCase::MalformedFrames => malformed_frame_scenarios::run(server.base_url()).await,
    };
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}
