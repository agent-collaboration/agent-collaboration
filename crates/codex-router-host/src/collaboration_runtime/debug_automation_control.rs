//! Opt-in configuration of the debug automation fault controller.
use collaboration_runtime::DebugAutomationFaultController;
use std::{io, sync::Arc};

pub(super) fn configured_faults() -> io::Result<Option<Arc<DebugAutomationFaultController>>> {
    match std::env::var("CODEX_ROUTER_DEBUG_AUTOMATION_FAULTS") {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Ok(value) if value == "1" => Ok(Some(Arc::new(DebugAutomationFaultController::default()))),
        _ => Err(io::Error::other(
            "CODEX_ROUTER_DEBUG_AUTOMATION_FAULTS must be 1 when set",
        )),
    }
}
