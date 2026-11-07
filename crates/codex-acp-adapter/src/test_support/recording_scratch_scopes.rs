//! Records the scratch scope opens and closes session bindings report.
use codex_acp_adapter::ScratchScopeRegister;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScratchScopeEvent {
    Opened(PathBuf),
    Closed(PathBuf),
}

#[derive(Default)]
pub struct RecordingScratchScopes {
    events: Mutex<Vec<ScratchScopeEvent>>,
}

impl RecordingScratchScopes {
    pub fn events(&self) -> Vec<ScratchScopeEvent> {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl ScratchScopeRegister for RecordingScratchScopes {
    fn opened(&self, scope_folder: &Path) {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(ScratchScopeEvent::Opened(scope_folder.to_owned()));
    }

    fn closed(&self, scope_folder: &Path) {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(ScratchScopeEvent::Closed(scope_folder.to_owned()));
    }
}
