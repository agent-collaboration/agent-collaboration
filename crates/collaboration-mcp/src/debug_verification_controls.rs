//! Named verification controls share the Host's owner-only socket and API lifecycle.
use axum::Router;
use collaboration_runtime::DebugAutomationFaultController;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

mod automation_faults;
mod verification_clock;

/// Controllers supplied by debug Host composition. Missing controllers expose no routes.
/// The verification-clock owner adds its optional controller alongside the fault controller.
#[derive(Default)]
pub struct DebugVerificationControls {
    pub automation_faults: Option<Arc<DebugAutomationFaultController>>,
    pub verification_clock: Option<collaboration_service::SubscriptionDeliveryService>,
}

pub(crate) fn router(controls: DebugVerificationControls, shutdown: CancellationToken) -> Router {
    let mut router = Router::new();
    if let Some(faults) = controls.automation_faults {
        router = router.merge(automation_faults::router(faults, shutdown.clone()));
    }
    if let Some(clock) = controls.verification_clock {
        router = router.merge(verification_clock::router(clock, shutdown));
    }
    router
}

#[cfg(test)]
mod tests;
