//! Named verification controls share the Host's owner-only socket and API lifecycle.
use axum::Router;
use collaboration_runtime::DebugAutomationFaultController;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

mod automation_faults;

/// Controllers supplied by debug Host composition. Missing controllers expose no routes.
/// The verification-clock owner adds its optional controller alongside the fault controller.
#[derive(Default)]
pub struct DebugVerificationControls {
    pub automation_faults: Option<Arc<DebugAutomationFaultController>>,
}

pub(crate) fn router(controls: DebugVerificationControls, shutdown: CancellationToken) -> Router {
    match controls.automation_faults {
        Some(faults) => automation_faults::router(faults, shutdown),
        None => Router::new(),
    }
}

#[cfg(test)]
mod tests;
