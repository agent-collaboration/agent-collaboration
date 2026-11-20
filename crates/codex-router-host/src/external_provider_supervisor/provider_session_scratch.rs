//! The scratch folder a Claude or Cursor session is created with (R4).
//!
//! Each create gets `<router root>/scratch/session-<uuidv7>`, prepared the same
//! way as a Codex session's (owner-private, stamped as used, under the scope
//! lock the Host's sweep honours). The folder is not registered as open: as
//! `collaboration_runtime::OpenScratchScopes` records, these sessions are kept
//! by the use stamp and the folder's newest change.

use std::path::{Path, PathBuf};

/// Prepares a fresh session folder under `scratch_root`; `Ok(None)` when this
/// supervisor grants no folder.
pub(super) async fn prepare_provider_session_scratch(
    scratch_root: Option<&Path>,
) -> Result<Option<PathBuf>, &'static str> {
    let Some(scratch_root) = scratch_root else {
        return Ok(None);
    };
    let scope = collaboration_runtime::new_service_uuid()
        .map_err(|_| "session scratch scope could not be minted")?;
    let folder = scratch_root.join(format!("session-{}", String::from(scope)));
    let prepared = folder.clone();
    tokio::task::spawn_blocking(move || codex_native_integration::prepare_scratch_scope(&prepared))
        .await
        .map_err(|_| "session scratch folder preparation did not complete")?
        .map_err(|_| "session scratch folder could not be prepared")?;
    Ok(Some(folder))
}

#[cfg(test)]
#[path = "provider_session_scratch_test.rs"]
mod tests;
