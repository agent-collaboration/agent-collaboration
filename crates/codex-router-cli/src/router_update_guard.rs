//! Refuses to start the Host, or run an account command, on a Router root that still
//! needs `agent-router-update`. This runs before anything opens or creates a store.
use crate::account::AccountCommand;
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

/// Account commands run synchronously off the async runtime, so the check gets its own.
pub(crate) fn refuse_account_command_needing_update(
    command: &AccountCommand,
) -> Result<(), RouterUpdateGuardError> {
    let router_root = match command {
        AccountCommand::Help(_) => return Ok(()),
        AccountCommand::Remove { router_root, .. }
        | AccountCommand::Login { router_root, .. }
        | AccountCommand::List { router_root }
        | AccountCommand::SetStatus { router_root, .. }
        | AccountCommand::SetWeeklyFloor { router_root, .. } => router_root,
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(RouterUpdateGuardError::Runtime)?
        .block_on(refuse_root_needing_update(router_root))
}
