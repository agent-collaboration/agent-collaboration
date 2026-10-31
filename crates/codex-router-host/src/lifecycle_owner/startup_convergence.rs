//! Ordered router and app-server startup convergence with retained-child cleanup.

use super::{
    AppServerChild, AppServerLaunchPlan, AppServerReadiness, ChildCommandSpec, HostConfig,
    HostError, RouterChild, RouterCondition, RouterProbeError, RouterProbeResult, probe_router,
};
use codex_router_secret_store::{
    file_backend::FileSecretStore,
    local_router_token::{LocalRouterTokenError, LocalRouterTokenService},
};
use std::path::Path;

pub(super) async fn start_router(
    config: &HostConfig,
    router_command: Option<&ChildCommandSpec>,
) -> Result<(RouterCondition, Option<RouterChild>), HostError> {
    let started_at = std::time::Instant::now();
    match probe_router(config.router_endpoint(), config.deadlines().router_start()).await? {
        RouterProbeResult::Compatible => {
            crate::record_debug_readiness_timing("routerReady", started_at);
            Ok((RouterCondition::ExternalReachable, None))
        }
        RouterProbeResult::AuthenticationRequired => Err(HostError::RouterAuthenticationRequired),
        RouterProbeResult::Incompatible => Err(HostError::RouterIncompatible),
        RouterProbeResult::Unavailable => {
            let command = router_command.ok_or(HostError::RouterUnavailable)?;
            let secret_root = config
                .coordination_paths()
                .router_secret_root()
                .ok_or(HostError::RouterSecretRootUnavailable)?;
            provision_local_router_token(&secret_root)?;
            crate::router_credential_migration::migrate_before_router_spawn(&secret_root).await;
            let mut command = command.command();
            let mut child = RouterChild::spawn(&mut command)?;
            #[cfg(debug_assertions)]
            let readiness_started = std::time::Instant::now();
            let probe_result = match await_router_readiness(
                config.router_endpoint(),
                config.deadlines().router_start(),
            )
            .await
            {
                Ok(result) => {
                    #[cfg(debug_assertions)]
                    record_router_startup_observation(&mut child, readiness_started, Some(result));
                    result
                }
                Err(error) => {
                    #[cfg(debug_assertions)]
                    record_router_startup_observation(&mut child, readiness_started, None);
                    let _shutdown_outcome = child.shutdown().await?;
                    return Err(HostError::RouterProbe(error));
                }
            };
            match probe_result {
                RouterProbeResult::Compatible => {
                    crate::record_debug_readiness_timing("routerReady", started_at);
                    Ok((RouterCondition::OwnedReachable, Some(child)))
                }
                RouterProbeResult::AuthenticationRequired => {
                    let _shutdown_outcome = child.shutdown().await?;
                    Err(HostError::RouterAuthenticationRequired)
                }
                RouterProbeResult::Incompatible => {
                    let _shutdown_outcome = child.shutdown().await?;
                    Err(HostError::RouterIncompatible)
                }
                RouterProbeResult::Unavailable => {
                    let _shutdown_outcome = child.shutdown().await?;
                    Err(HostError::RouterUnavailable)
                }
            }
        }
    }
}

#[cfg(debug_assertions)]
fn record_router_startup_observation(
    child: &mut RouterChild,
    started: std::time::Instant,
    readiness: Option<RouterProbeResult>,
) {
    use std::os::unix::process::ExitStatusExt;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let child_pid = child.process_id();
    match child.observe_startup_status() {
        Ok(Some(status)) => tracing::debug!(target: "codex_router_host::startup_diagnostics", {
            event.name = "codex_router.host.router_startup_child",
            stage = "router_readiness",
            child_state = "exited",
            child_pid,
            exit_code = ?status.code(),
            exit_signal = ?status.signal(),
            readiness = ?readiness,
            elapsed_ms,
            }, "Observed retained Router child"
        ),
        Ok(None) => tracing::debug!(target: "codex_router_host::startup_diagnostics", {
            event.name = "codex_router.host.router_startup_child",
            stage = "router_readiness",
            child_state = "running",
            child_pid,
            readiness = ?readiness,
            elapsed_ms,
            }, "Observed retained Router child"
        ),
        Err(_) => tracing::debug!(target: "codex_router_host::startup_diagnostics", {
            event.name = "codex_router.host.router_startup_child",
            stage = "router_readiness",
            child_state = "observation_failed",
            child_pid,
            readiness = ?readiness,
            elapsed_ms,
            }, "Retained Router status unavailable"
        ),
    }
}

fn provision_local_router_token(secret_root: &Path) -> Result<(), LocalRouterTokenError> {
    let store = FileSecretStore::open(secret_root)?;
    LocalRouterTokenService::new(store).ensure_local_token(secret_root)?;
    Ok(())
}

async fn await_router_readiness(
    endpoint: std::net::SocketAddr,
    deadline: std::time::Duration,
) -> Result<RouterProbeResult, RouterProbeError> {
    let deadline_at = tokio::time::Instant::now() + deadline;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline_at {
            return Ok(RouterProbeResult::Unavailable);
        }
        match probe_router(endpoint, deadline_at.saturating_duration_since(now)).await? {
            RouterProbeResult::Unavailable => {
                tokio::time::sleep_until(
                    deadline_at
                        .min(tokio::time::Instant::now() + std::time::Duration::from_millis(20)),
                )
                .await;
            }
            terminal => return Ok(terminal),
        }
    }
}

pub(super) async fn start_app_server(
    config: &HostConfig,
    mut launch_plan: AppServerLaunchPlan,
) -> Result<(AppServerChild, AppServerReadiness), HostError> {
    let started_at = std::time::Instant::now();
    let spawn_started_at = std::time::Instant::now();
    let mut child = launch_plan.spawn()?;
    crate::record_debug_readiness_timing("appServerSpawn", spawn_started_at);
    let readiness = child
        .await_readiness(
            config.app_server_socket(),
            config.deadlines().app_server_start(),
            config.deadlines().remote_control(),
        )
        .await;
    match readiness {
        Ok(readiness) => {
            // Schema export is optional and runs only after the native socket is accepting.
            // A failed export retains raw-native access without delaying socket startup.
            launch_plan.prepare_schema().await;
            child.set_schema_export(launch_plan.prepared_schema_export());
            crate::record_debug_readiness_timing("appServerSpawnAndReady", started_at);
            Ok((child, readiness))
        }
        Err(readiness_error) => {
            let _shutdown_outcome = child.shutdown().await?;
            Err(HostError::AppServerReadiness(readiness_error))
        }
    }
}

pub(super) async fn shutdown_owned_router_after_startup_failure(
    router: &mut Option<RouterChild>,
) -> Result<(), HostError> {
    if let Some(child) = router.as_mut() {
        let _shutdown_outcome = child.shutdown().await?;
    }
    Ok(())
}

#[cfg(test)]
#[cfg(debug_assertions)]
#[path = "startup_diagnostic_tests.rs"]
mod diagnostic_tests;

#[cfg(test)]
#[path = "startup_convergence_test.rs"]
mod tests;
