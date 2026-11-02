//! The shared import scenario runs against an owning native Fleet store.
#![cfg(all(feature = "conformance", debug_assertions))]
#[path = "native_http/support.rs"]
mod configuration;
use collaboration_central_conformance::{
    AutomationClockControl, ConformanceClient, ConformanceResult, JwksFixtureServer,
    RuntimeControl, RuntimeControlRequest, fixture_policy,
    run_automation_handover_carried_conformance, run_automation_handover_conformance,
    run_automation_handover_external_conformance, run_automation_handover_lifecycle_conformance,
    run_automation_handover_wake_conformance,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{DeliveryConfig, TokioServer, TokioSettings};

async fn start(root: &std::path::Path, jwks: &str) -> ConformanceResult<TokioServer> {
    Ok(TokioServer::start_with_controls(
        TokioSettings {
            root: root.into(),
            delivery: DeliveryConfig {
                relay_peer: "019f0000-0000-7000-8000-000000005240"
                    .to_owned()
                    .try_into()?,
            },
            access_policy: fixture_policy(),
            jwks_url: jwks.into(),
            registry: configuration::registry_config()?,
            holder_answer_timeout: std::time::Duration::from_secs(2),
            copy_ack_wait: std::time::Duration::from_secs(2),
        },
        vec![RuntimeControlRequest {
            address: ObjectAddress::Fleet,
            command: RuntimeControl::AutomationClock(AutomationClockControl::Start {
                now: RegistryTime("2026-10-31T10:00:00Z".parse()?),
            }),
        }],
    )
    .await?)
}

#[tokio::test]
async fn imported_lifecycle_and_per_definition_phase_never_refire_local_work()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = start(root.path(), &jwks.url).await?;
    let proof =
        run_automation_handover_lifecycle_conformance(&ConformanceClient::new(server.base_url())?)
            .await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    stopped?;
    keys?;
    proof
}

#[tokio::test]
async fn carried_wake_reports_belong_to_the_importer_without_redispatch() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = start(root.path(), &jwks.url).await?;
    let proof =
        run_automation_handover_wake_conformance(&ConformanceClient::new(server.base_url())?).await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    stopped?;
    keys?;
    proof
}

#[tokio::test]
async fn frozen_handover_is_atomic_and_bound_to_its_authenticated_importer() -> ConformanceResult<()>
{
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = start(root.path(), &jwks.url).await?;
    let proof =
        run_automation_handover_conformance(&ConformanceClient::new(server.base_url())?).await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    stopped?;
    keys?;
    proof
}

#[tokio::test]
async fn suspended_handover_releases_owed_work_once_at_its_fitted_due() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = start(root.path(), &jwks.url).await?;
    let proof =
        run_automation_handover_carried_conformance(&ConformanceClient::new(server.base_url())?)
            .await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    stopped?;
    keys?;
    proof
}

#[tokio::test]
async fn imported_external_occupants_release_retained_waiters_at_their_due() -> ConformanceResult<()>
{
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = start(root.path(), &jwks.url).await?;
    let proof =
        run_automation_handover_external_conformance(&ConformanceClient::new(server.base_url())?)
            .await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    stopped?;
    keys?;
    proof
}

#[tokio::test]
async fn imported_external_occupants_block_successors_across_later_dues() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = start(root.path(), &jwks.url).await?;
    let proof =
        collaboration_central_conformance::run_automation_handover_external_due_conformance(
            &ConformanceClient::new(server.base_url())?,
        )
        .await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    proof?;
    stopped?;
    keys?;
    Ok(())
}

async fn restart_handover_case(waiting_due_now: bool) -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let proof: ConformanceResult<()> = async {
        let server = start(root.path(), &jwks.url).await?;
        let prepared = collaboration_central_conformance::prepare_automation_handover_restart(
            &ConformanceClient::new(server.base_url())?,
            waiting_due_now,
        )
        .await;
        let stopped = server.shutdown().await;
        let prepared = prepared?;
        stopped?;
        let server = start(root.path(), &jwks.url).await?;
        let proof = collaboration_central_conformance::resume_automation_handover_restart(
            &ConformanceClient::new(server.base_url())?,
            prepared,
        )
        .await;
        let stopped = server.shutdown().await;
        proof?;
        stopped?;
        Ok(())
    }
    .await;
    let keys = jwks.shutdown().await;
    proof?;
    keys?;
    Ok(())
}

#[tokio::test]
async fn future_carried_handover_restart_reads_commit_before_retry() -> ConformanceResult<()> {
    restart_handover_case(false).await
}

#[tokio::test]
async fn due_now_carried_handover_restart_reads_commit_before_retry() -> ConformanceResult<()> {
    restart_handover_case(true).await
}

#[tokio::test]
async fn frozen_inner_payload_limits_and_stored_reference_failures_are_atomic()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = start(root.path(), &jwks.url).await?;
    let proof = collaboration_central_conformance::run_automation_handover_validation_conformance(
        &ConformanceClient::new(server.base_url())?,
    )
    .await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    proof?;
    stopped?;
    keys?;
    Ok(())
}

#[tokio::test]
async fn suspended_definitions_repair_through_existing_schedule_and_wake_mutations()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = start(root.path(), &jwks.url).await?;
    let proof = collaboration_central_conformance::run_automation_handover_repair_conformance(
        &ConformanceClient::new(server.base_url())?,
    )
    .await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    proof?;
    stopped?;
    keys?;
    Ok(())
}
