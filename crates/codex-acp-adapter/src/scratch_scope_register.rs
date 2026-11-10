//! The Host's record of which scratch scopes have a Codex session open.
//!
//! A session binding that carries a Router access route holds a
//! [`ScratchScopeLease`] on its scratch folder for as long as it lives: it is
//! taken when the binding is created, loaded or resumed and released when the
//! binding is dropped. The Host's scratch sweep skips every folder with an
//! open lease. The register is the Host's; this crate only reports to it.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Receives opens and closes of scratch scopes from session bindings.
///
/// Calls arrive in pairs per lease; two bindings on the same folder (two ACP
/// connections) report two opens and keep the folder open until both close.
pub trait ScratchScopeRegister: Send + Sync {
    /// A binding on `scope_folder` was created, loaded or resumed.
    fn opened(&self, scope_folder: &Path);
    /// A binding that reported `opened` for `scope_folder` was dropped.
    fn closed(&self, scope_folder: &Path);
}

/// One binding's open use of a scratch folder; reports the close when dropped.
pub struct ScratchScopeLease {
    register: Arc<dyn ScratchScopeRegister>,
    scope_folder: PathBuf,
}

impl ScratchScopeLease {
    #[must_use]
    pub fn open(register: &Arc<dyn ScratchScopeRegister>, scope_folder: &Path) -> Self {
        register.opened(scope_folder);
        Self {
            register: Arc::clone(register),
            scope_folder: scope_folder.to_owned(),
        }
    }
}

impl Drop for ScratchScopeLease {
    fn drop(&mut self) {
        self.register.closed(&self.scope_folder);
    }
}

impl std::fmt::Debug for ScratchScopeLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScratchScopeLease")
            .field("scope_folder", &self.scope_folder)
            .finish_non_exhaustive()
    }
}
