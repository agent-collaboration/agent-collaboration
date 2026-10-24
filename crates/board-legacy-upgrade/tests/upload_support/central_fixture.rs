//! Real local central configuration shared by importer route tests.
use collaboration_central_conformance::{JwksFixtureServer, fixture_policy, owner_assertion};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use fleet_sync_protocol::ReceiverId;
use reqwest::header::{HeaderMap, HeaderValue};
use std::time::Duration;
type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

pub async fn start_server(
    root: &std::path::Path,
    jwks: &JwksFixtureServer,
) -> TestResult<TokioServer> {
    Ok(TokioServer::start(TokioSettings {
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
    .await?)
}
pub fn owner_client() -> TestResult<reqwest::Client> {
    let mut headers = HeaderMap::new();
    let mut assertion = HeaderValue::from_str(
        &owner_assertion().map_err(|error| error as Box<dyn std::error::Error>)?,
    )?;
    assertion.set_sensitive(true);
    headers.insert("Cf-Access-Jwt-Assertion", assertion);
    Ok(reqwest::Client::builder()
        .no_proxy()
        .default_headers(headers)
        .timeout(Duration::from_secs(10))
        .build()?)
}
