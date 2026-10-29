//! Legacy Access obligations run through Tokio and the shared HTTP verifier.
#![cfg(feature = "conformance")]

use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, fixture_jwks,
    fixture_key_profile_jwks, fixture_policy, run_access_expiry_conformance,
    run_access_key_profile_conformance, run_access_verifier_control_conformance,
    run_access_zero_age_conformance, run_shared_access_conformance,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::time::Duration;
#[path = "support/clocked_server.rs"]
mod clocked_server;
#[path = "native_http/support.rs"]
mod configuration;

enum AccessCase {
    ZeroCacheAge,
    KeyProfile,
    VerifierControl,
    HttpSessions,
}

async fn run_case(case: AccessCase) -> ConformanceResult<()> {
    let keys = match case {
        AccessCase::ZeroCacheAge => {
            JwksFixtureServer::start_with_cache_control(fixture_jwks()?, "public, max-age=0")
                .await?
        }
        AccessCase::KeyProfile => {
            JwksFixtureServer::start_with_keys(fixture_key_profile_jwks()?).await?
        }
        AccessCase::VerifierControl | AccessCase::HttpSessions => {
            JwksFixtureServer::start().await?
        }
    };
    let directory = tempfile::tempdir()?;
    let server = TokioServer::start(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: directory.path().into(),
        access_policy: fixture_policy(),
        jwks_url: keys.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let client = ConformanceClient::new(server.base_url())?;
    let result = async {
        match case {
            AccessCase::ZeroCacheAge => {
                run_access_zero_age_conformance(&client, &keys.requests).await
            }
            AccessCase::KeyProfile => run_access_key_profile_conformance(&client).await,
            AccessCase::VerifierControl => run_access_verifier_control_conformance(&client).await,
            AccessCase::HttpSessions => {
                run_shared_access_conformance(&client, &keys.requests).await
            }
        }
    }
    .await;
    let stopped = server.shutdown().await;
    let fixture = keys.shutdown().await;
    stopped?;
    fixture?;
    result
}

#[tokio::test]
async fn an_upstream_zero_cache_lifetime_is_preserved_through_real_native_verification()
-> ConformanceResult<()> {
    run_case(AccessCase::ZeroCacheAge).await
}

#[tokio::test]
async fn valid_2048_and_tampered_controls_do_not_admit_a_valid_1024_bit_signature()
-> ConformanceResult<()> {
    run_case(AccessCase::KeyProfile).await
}

#[tokio::test]
async fn verifier_control_runs_the_edge_verifier_on_submitted_assertions() -> ConformanceResult<()>
{
    run_case(AccessCase::VerifierControl).await
}

#[tokio::test]
async fn expiry_row_refuses_the_same_assertion_after_its_exp() -> ConformanceResult<()> {
    let keys = JwksFixtureServer::start().await?;
    let directory = tempfile::tempdir()?;
    let server = clocked_server::ClockedServer::start(directory.path(), &keys.url).await?;
    let client = ConformanceClient::new(server.base_url())?;
    let now = chrono::Utc::now();
    server
        .clock
        .set_time(collaboration_central_core::RegistryTime(now));
    let clock = server.clock.clone();
    let result = run_access_expiry_conformance(&client, now, |at| async move {
        clock.set_time(collaboration_central_core::RegistryTime(at));
        Ok(())
    })
    .await;
    let stopped = server.shutdown().await;
    let fixture = keys.shutdown().await;
    stopped?;
    fixture?;
    result
}

#[tokio::test]
async fn signed_access_assertions_gate_real_native_http_and_websockets() -> ConformanceResult<()> {
    run_case(AccessCase::HttpSessions).await
}
