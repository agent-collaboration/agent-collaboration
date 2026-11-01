#![cfg(feature = "conformance")]

use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, fixture_policy,
    run_admitted_session_conformance, run_enrollment_identity_conformance,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
#[path = "support/copy_drive_overlap.rs"]
mod copy_drive_overlap;
#[path = "support/holder_mapping_scenarios.rs"]
mod holder_mapping_scenarios;

#[tokio::test]
async fn a_missing_committed_holder_mapping_never_dispatches_to_an_object() -> ConformanceResult<()>
{
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
        registry: support::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let result = holder_mapping_scenarios::run(server.base_url()).await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/copy_wait_scenarios.rs"]
mod copy_wait_scenarios;
#[path = "support/holder_full_wait.rs"]
mod holder_full_wait;
#[path = "support/holder_writer_retry.rs"]
mod holder_writer_retry;

#[tokio::test]
async fn sequential_release_and_copy_wait_keep_live_drive_replay_pending() -> ConformanceResult<()>
{
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
        registry: support::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(20),
        copy_ack_wait: std::time::Duration::from_secs(20),
    })
    .await?;
    let service = message_board::ServiceId::try_from(copy_drive_overlap::SERVICE.to_owned())?;
    let result = copy_drive_overlap::run(server.base_url(), || async {
        Ok(server
            .conformance_platform()
            .conformance_holder_frames(&service)
            .await?)
    })
    .await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}

#[tokio::test]
async fn full_holder_attempt_finishes_inside_the_original_drive_lease_without_waiter_overlap()
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
        registry: support::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(20),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let service = message_board::ServiceId::try_from(holder_full_wait::SERVICE.to_owned())?;
    let result = holder_full_wait::run(server.base_url(), || async {
        Ok(server
            .conformance_platform()
            .conformance_holder_frames(&service)
            .await?)
    })
    .await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}
#[path = "native_http/support.rs"]
mod support;

#[tokio::test]
async fn lost_holder_answers_preserve_the_call_and_real_writer_receipts_across_reopen()
-> ConformanceResult<()> {
    let jwks = JwksFixtureServer::start().await?;
    let directory = tempfile::tempdir()?;
    let central = directory.path().join("central");
    tokio::fs::create_dir_all(&central).await?;
    let server = TokioServer::start_conformance(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: central,
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: support::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let result =
        holder_writer_retry::run(server.base_url(), &directory.path().join("writer.db")).await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}

#[tokio::test]
async fn registry_holder_calls_reach_the_other_enrollment_and_accept_separate_socket_answers()
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
        registry: support::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let client = ConformanceClient::new(server.base_url())?;
    let result = collaboration_central_conformance::run_holder_reply_conformance(&client).await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}

#[tokio::test]
async fn renewal_is_correlated_through_the_real_registry_and_keeps_the_session_usable()
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
        registry: support::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let client = ConformanceClient::new(server.base_url())?;
    let result = collaboration_central_conformance::run_lease_renewal_conformance(&client).await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}

#[tokio::test]
async fn enrollment_returns_a_stable_peer_and_expiry_over_native_http() -> ConformanceResult<()> {
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
        registry: support::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let client = ConformanceClient::new(server.base_url())?;
    let result = run_enrollment_identity_conformance(&client).await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}

#[tokio::test]
async fn admitted_v4_service_completes_the_real_schema_publication_before_welcome()
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
        registry: support::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let client = ConformanceClient::new(server.base_url())?;
    let service =
        message_board::ServiceId::try_from("550e8400-e29b-41d4-a716-446655440001".to_owned())?;
    let result = run_admitted_session_conformance(&client, |peer| {
        observe_admission(&server, &service, peer)
    })
    .await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result?;
    if !directory
        .path()
        .join("connection-550e8400-e29b-41d4-a716-446655440001.db")
        .is_file()
    {
        return Err("upgrade did not route to the authenticated ServiceId object".into());
    }
    Ok(())
}

async fn observe_admission(
    server: &TokioServer,
    service: &message_board::ServiceId,
    peer: durable_steps::Uuid,
) -> ConformanceResult<()> {
    let origins = server
        .conformance_platform()
        .conformance_socket_origins(service)
        .await?;
    if origins.len() != 1
        || origins.first().is_none_or(|origin| {
            origin.installation != *service || origin.machine.as_str() != "original admission label"
        })
    {
        return Err(
            "real upgraded socket did not retain its admission origin after relabeling".into(),
        );
    }
    let report = server
        .conformance_platform()
        .conformance_schema_report(fleet_sync_protocol::PeerId::new(peer)?)
        .await?;
    if serde_json::to_value(report)?
        != serde_json::json!({"peer":peer,"sessionId":"019f0000-0000-7000-8000-000000000450","attachSeq":1,"support":{"maxVersion":1}})
    {
        return Err("Welcome did not follow the exact committed Registry schema report".into());
    }
    Ok(())
}
