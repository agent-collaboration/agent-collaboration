//! Central remote-create custody runs through normal authenticated machine sessions.
#![cfg(all(feature = "conformance", debug_assertions))]
#[path = "native_http/support.rs"]
mod configuration;
use collaboration_central_conformance::{
    AutomationClockControl, ConformanceClient, ConformanceResult, JwksFixtureServer,
    RuntimeControl, RuntimeControlRequest, fixture_policy, run_remote_session_create_conformance,
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

fn startup() -> ConformanceResult<Vec<RuntimeControlRequest>> {
    Ok(vec![RuntimeControlRequest {
        address: ObjectAddress::Fleet,
        command: RuntimeControl::AutomationClock(AutomationClockControl::Start {
            now: RegistryTime("2026-10-31T10:00:01.123Z".parse()?),
        }),
    }])
}

#[tokio::test]
async fn central_remote_create_preserves_custody_and_target_report_ownership()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server =
        TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup()?).await?;
    let proof =
        run_remote_session_create_conformance(&ConformanceClient::new(server.base_url())?).await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    stopped?;
    keys?;
    proof
}

#[tokio::test]
async fn retained_remote_request_and_report_owner_survive_prune_and_store_reopen()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let outcome: ConformanceResult<()> = async {
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup()?).await?;
        let prepared = collaboration_central_conformance::prepare_remote_create_restart(
            &ConformanceClient::new(server.base_url())?,
        )
        .await;
        let stopped = server.shutdown().await;
        let prepared = prepared?;
        stopped?;
        let server =
            TokioServer::start_with_controls(settings(root.path(), &jwks.url)?, startup()?).await?;
        let proof = collaboration_central_conformance::resume_remote_create_restart(
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
