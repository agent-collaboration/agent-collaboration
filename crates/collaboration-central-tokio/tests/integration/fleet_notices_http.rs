//! A committed Fleet mutation reaches its reader through the real alarm and Delivery.
#![cfg(feature = "conformance")]
use crate::configuration;
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, fixture_policy};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::time::Duration;
#[path = "../support/fleet_notice_scenarios.rs"]
mod scenarios;

#[tokio::test]
async fn committed_feed_advancement_delivers_one_positioned_notice_to_its_reader()
-> ConformanceResult<()> {
    run(false).await
}

#[tokio::test]
async fn matched_client_protocol_keeps_mail_after_one_fleet_write() -> ConformanceResult<()> {
    run(true).await
}

async fn run(pairing: bool) -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(TokioSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof = if pairing {
        scenarios::one_write_then_mail(server.base_url()).await
    } else {
        scenarios::feed_advancement_reaches_reader_once(server.base_url()).await
    };
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    proof
}
