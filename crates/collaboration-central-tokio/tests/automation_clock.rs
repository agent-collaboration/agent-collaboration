//! Timer-boundary proof; Automation admission is covered by the shared clock scenarios.
#![cfg(all(feature = "conformance", debug_assertions))]

#[path = "native_http/support.rs"]
mod configuration;
use collaboration_central_conformance::{
    AutomationClockControl, ConformanceClient, ConformanceResult, JwksFixtureServer,
    RuntimeControl, RuntimeControlRequest, fixture_policy,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{DeliveryConfig, TokioServer, TokioSettings};

fn settings(root: &std::path::Path, jwks: &str) -> ConformanceResult<TokioSettings> {
    Ok(TokioSettings {
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
    })
}
fn startup(now: RegistryTime) -> Vec<RuntimeControlRequest> {
    vec![RuntimeControlRequest {
        address: ObjectAddress::Fleet,
        command: RuntimeControl::AutomationClock(AutomationClockControl::Start { now }),
    }]
}

#[tokio::test]
async fn catalog_clock_batches_and_custody_survive_faults() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let outcome: ConformanceResult<()> = async {
        let initial = RegistryTime("2026-10-31T10:04:59.999Z".parse()?);
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup(initial))
                .await?;
        let proof = collaboration_central_conformance::run_automation_clock_conformance(
            &ConformanceClient::new(server.base_url())?,
            collaboration_central_conformance::AutomationClockPlatform::Tokio,
        )
        .await;
        let stopped = server.shutdown().await;
        proof?;
        stopped?;
        Ok(())
    }
    .await;
    let keys = jwks.shutdown().await;
    outcome?;
    keys?;
    Ok(())
}

#[tokio::test]
async fn report_owner_survives_acknowledged_batch_retention() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let outcome: ConformanceResult<()> = async {
        let initial = RegistryTime("2026-10-31T10:04:59.999Z".parse()?);
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup(initial))
                .await?;
        let proof = collaboration_central_conformance::run_automation_retention_conformance(
            &ConformanceClient::new(server.base_url())?,
        )
        .await;
        let stopped = server.shutdown().await;
        proof?;
        stopped?;
        Ok(())
    }
    .await;
    let keys = jwks.shutdown().await;
    outcome?;
    keys?;
    Ok(())
}

#[tokio::test]
async fn registered_expired_project_lease_does_not_gate_automation() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let outcome: ConformanceResult<()> = async {
        let initial = RegistryTime("2030-01-01T00:00:00Z".parse()?);
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup(initial))
                .await?;
        let proof = collaboration_central_conformance::run_automation_expired_lease_conformance(
            &ConformanceClient::new(server.base_url())?,
            collaboration_central_conformance::AutomationClockPlatform::Tokio,
        )
        .await;
        let stopped = server.shutdown().await;
        proof?;
        stopped?;
        Ok(())
    }
    .await;
    let keys = jwks.shutdown().await;
    outcome?;
    keys?;
    Ok(())
}

#[tokio::test]
async fn paused_and_faulted_wakes_keep_independent_expiry() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let outcome: ConformanceResult<()> = async {
        let initial = RegistryTime("2026-10-31T10:04:59.999Z".parse()?);
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup(initial))
                .await?;
        let proof = collaboration_central_conformance::run_automation_expiry_conformance(
            &ConformanceClient::new(server.base_url())?,
        )
        .await;
        let stopped = server.shutdown().await;
        proof?;
        stopped?;
        Ok(())
    }
    .await;
    let keys = jwks.shutdown().await;
    outcome?;
    keys?;
    Ok(())
}

#[tokio::test]
async fn armed_restart_then_drain_and_create_uses_actual_callbacks() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let outcome: ConformanceResult<()> = async {
        let initial = RegistryTime("2026-10-31T10:04:59.999Z".parse()?);
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup(initial))
                .await?;
        let prepared = collaboration_central_conformance::prepare_automation_restart(
            &ConformanceClient::new(server.base_url())?,
        )
        .await;
        let stopped = server.shutdown().await;
        let prepared = prepared?;
        stopped?;
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup(initial))
                .await?;
        let proof = collaboration_central_conformance::resume_automation_restart(
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
    outcome?;
    keys?;
    Ok(())
}

#[tokio::test]
async fn external_settlement_releases_the_validated_pending_edit_after_body_fault()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let outcome: ConformanceResult<()> = async {
        let initial = RegistryTime("2026-10-31T10:04:59.999Z".parse()?);
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup(initial))
                .await?;
        let proof = collaboration_central_conformance::run_automation_external_conformance(
            &ConformanceClient::new(server.base_url())?,
        )
        .await;
        let stopped = server.shutdown().await;
        proof?;
        stopped?;
        Ok(())
    }
    .await;
    let keys = jwks.shutdown().await;
    outcome?;
    keys?;
    Ok(())
}

#[tokio::test]
async fn central_entry_and_stored_rows_enforce_the_limit_inventory() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let outcome: ConformanceResult<()> = async {
        let initial = RegistryTime("2026-10-31T10:00:00Z".parse()?);
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup(initial))
                .await?;
        let proof = collaboration_central_conformance::run_automation_limit_conformance(
            &ConformanceClient::new(server.base_url())?,
        )
        .await;
        let stopped = server.shutdown().await;
        proof?;
        stopped?;
        Ok(())
    }
    .await;
    let keys = jwks.shutdown().await;
    outcome?;
    keys?;
    Ok(())
}

#[tokio::test]
async fn undelivered_wake_folds_without_changing_its_frozen_header() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let outcome: ConformanceResult<()> = async {
        let initial = RegistryTime("2026-10-31T10:04:59.999Z".parse()?);
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup(initial))
                .await?;
        let proof = collaboration_central_conformance::run_automation_wake_delivery_conformance(
            &ConformanceClient::new(server.base_url())?,
        )
        .await;
        let stopped = server.shutdown().await;
        proof?;
        stopped?;
        Ok(())
    }
    .await;
    let keys = jwks.shutdown().await;
    outcome?;
    keys?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions are the native clock conformance oracle; server and fixture failures propagate as errors."
)]
async fn advance_drains_real_callbacks_and_restart_retains_time() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let outcome: ConformanceResult<()> = async {
        let initial = RegistryTime("2026-10-31T10:04:59.999Z".parse()?);
        let boundary = RegistryTime("2026-10-31T10:05:00Z".parse()?);
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup(initial))
                .await?;
        let client = ConformanceClient::new(server.base_url())?;
        let proof: ConformanceResult<()> = async {
            let before = client
                .automation_clock(AutomationClockControl::Observe)
                .await?;
            assert!(
                !before.callbacks.is_empty(),
                "startup callback was not drained"
            );
            assert!(before.next_alarm_ms.is_none());
            let quiet = client
                .automation_clock(AutomationClockControl::Advance { now: boundary })
                .await?;
            assert_eq!(quiet.now, boundary);
            assert_eq!(
                quiet.callbacks.len(),
                before.callbacks.len(),
                "quiet interval manufactured a callback"
            );
            let injected = client
                .automation_clock(AutomationClockControl::InvokeAlarm)
                .await?;
            assert_eq!(
                injected.callbacks.len(),
                quiet.callbacks.len() + 1,
                "actual injected callback was not drained"
            );
            assert!(injected.next_alarm_ms.is_none());
            let duplicate = client
                .automation_clock(AutomationClockControl::InvokeAlarm)
                .await?;
            assert_eq!(duplicate.callbacks.len(), injected.callbacks.len() + 1);
            Ok(())
        }
        .await;
        server.shutdown().await?;
        proof?;
        // The stale initial value cannot rewind the retained debug clock.
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup(initial))
                .await?;
        let proof = ConformanceClient::new(server.base_url())?
            .automation_clock(AutomationClockControl::Observe)
            .await;
        server.shutdown().await?;
        let reopened = proof?;
        assert_eq!(reopened.now, boundary);
        assert!(reopened.next_alarm_ms.is_none());
        let stored: RegistryTime = serde_json::from_slice(
            &tokio::fs::read(root.path().join("_automation-verification-clock.json")).await?,
        )?;
        assert_eq!(stored, boundary);
        Ok(())
    }
    .await;
    let keys = jwks.shutdown().await;
    outcome?;
    keys?;
    Ok(())
}
