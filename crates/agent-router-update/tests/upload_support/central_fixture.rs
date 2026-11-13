//! Real local central configuration shared by importer route tests.
use collaboration_central_conformance::{JwksFixtureServer, fixture_policy};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use fleet_sync_protocol::ReceiverId;
use std::time::Duration;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub async fn start_server(
    root: &std::path::Path,
    jwks: &JwksFixtureServer,
) -> TestResult<TokioServer> {
    Ok(TokioServer::start(settings(root, jwks)?).await?)
}

fn settings(root: &std::path::Path, jwks: &JwksFixtureServer) -> TestResult<TokioSettings> {
    Ok(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: RegistryConfig {
            lease: LeasePolicy {
                lease_duration: Duration::from_secs(14_400),
                renew_interval: Duration::from_secs(900),
            },
            authoritative_receiver: ReceiverId::try_from(
                "019f0000-0000-7000-8000-000000000010".to_owned(),
            )?,
            release_unsent_for: Duration::ZERO,
            move_needs_renewal_within: Some(Duration::from_secs(1800)),
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
}
