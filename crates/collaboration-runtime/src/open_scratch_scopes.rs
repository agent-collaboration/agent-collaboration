//! The Host's in-memory record of scratch scopes with a Codex session open.
//!
//! Session bindings report opens and closes through
//! [`codex_acp_adapter::ScratchScopeRegister`]; each report also records a use
//! in the folder (`.last-used`), so the folder's newest change reflects use
//! across restarts. The Host's scratch sweep skips every scope counted here.
//!
//! Limits, by design: the register starts empty at Host start, and scopes
//! reopened afterwards register again on load or resume; the 30-day rule on a
//! folder's newest change covers the gap. Claude Code and Cursor sessions do not
//! report here: their scratch folders are protected only by the use stamp
//! written when the folder is created and by the folder's newest change.

use codex_acp_adapter::ScratchScopeRegister;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Default)]
pub struct OpenScratchScopes {
    open: Mutex<BTreeMap<PathBuf, usize>>,
}

impl OpenScratchScopes {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether any session binding in this Host holds `scope_folder` open.
    #[must_use]
    pub fn is_open(&self, scope_folder: &Path) -> bool {
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(scope_folder)
    }

    fn record_use(scope_folder: &Path) {
        // A short filesystem step under the scope's shared lock; the sweep holds
        // the exclusive lock only while it rechecks and renames one folder.
        if let Err(error) = codex_native_integration::prepare_scratch_scope(scope_folder) {
            tracing::warn!(
                scope = %scope_folder.display(),
                %error,
                "scratch scope use could not be recorded"
            );
        }
    }
}

impl ScratchScopeRegister for OpenScratchScopes {
    fn opened(&self, scope_folder: &Path) {
        *self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(scope_folder.to_owned())
            .or_default() += 1;
        Self::record_use(scope_folder);
    }

    fn closed(&self, scope_folder: &Path) {
        {
            let mut open = self
                .open
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(count) = open.get_mut(scope_folder) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    open.remove(scope_folder);
                }
            }
        }
        Self::record_use(scope_folder);
    }
}

#[cfg(test)]
#[path = "open_scratch_scopes_test.rs"]
mod tests;
