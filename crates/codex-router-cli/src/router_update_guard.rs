//! Refuses to start the Host, or run an account command, on a Router root that still
//! needs `agent-router-update`. This runs before anything opens or creates a store.
//!
//! Callers pass the root; this module knows no command, so command modules depend on
//! it and never the other way round.
use codex_router_host::{
    RequiredRouterUpdate, RouterUpdateInspectionError, RouterUpdateRequirement,
    inspect_router_update_requirement,
};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum RouterUpdateGuardError {
    #[error("{0}")]
    UpdateRequired(RequiredRouterUpdate),
    #[error(transparent)]
    Inspection(#[from] RouterUpdateInspectionError),
    #[error("cannot start the update check: {0}")]
    Runtime(#[source] std::io::Error),
}

pub(crate) async fn refuse_root_needing_update(root: &Path) -> Result<(), RouterUpdateGuardError> {
    match inspect_router_update_requirement(root).await? {
        RouterUpdateRequirement::NotRequired => Ok(()),
        RouterUpdateRequirement::Required(required) => {
            Err(RouterUpdateGuardError::UpdateRequired(required))
        }
    }
}

/// For synchronous commands that run off the async runtime: the check gets its own.
pub(crate) fn refuse_root_needing_update_blocking(
    root: &Path,
) -> Result<(), RouterUpdateGuardError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(RouterUpdateGuardError::Runtime)?
        .block_on(refuse_root_needing_update(root))
}
