//! Frozen source identity and model metadata for one selected session action.
use super::{SessionPickerIdentity, SessionPickerRecord};
use codex_native_integration::ResumeModelChoice;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoredModelDiagnostic {
    UnquotableStoredValueDiscarded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionActionSelection {
    pub identity: SessionPickerIdentity,
    pub provenance: super::SessionRowProvenance,
    pub source_context: Option<crate::source_inventory_request::PickerSourceContext>,
    pub model_choice: ResumeModelChoice,
    pub model_diagnostic: Option<StoredModelDiagnostic>,
}

impl SessionActionSelection {
    pub fn from_picker_record(record: &SessionPickerRecord) -> Self {
        let model = record.model.as_deref();
        let effort = record.reasoning_effort.as_deref();
        let model_diagnostic = ResumeModelChoice::rejects_stored_value(model, effort)
            .then_some(StoredModelDiagnostic::UnquotableStoredValueDiscarded);
        Self {
            identity: record.identity.clone(),
            provenance: record.provenance,
            source_context: record.source_context.clone(),
            model_choice: ResumeModelChoice::from_stored_values(model, effort),
            model_diagnostic,
        }
    }

    pub fn default_catalog(session_id: String, model_choice: ResumeModelChoice) -> Self {
        Self {
            identity: SessionPickerIdentity::LocalCodex(session_id),
            provenance: super::SessionRowProvenance::LocalHomeCatalog,
            source_context: None,
            model_choice,
            model_diagnostic: None,
        }
    }

    /// Consumed only after the picker has returned and the dispatcher validated the session.
    pub fn emit_model_diagnostic(&self) {
        if let Some(StoredModelDiagnostic::UnquotableStoredValueDiscarded) = self.model_diagnostic {
            eprintln!(
                "agent-sessions: stored model or reasoning effort for session {} contains characters that cannot be passed to Codex; resuming without it",
                self.session_id()
            );
        }
    }

    pub fn session_id(&self) -> String {
        match &self.identity {
            SessionPickerIdentity::LocalCodex(session_id) => session_id.clone(),
            SessionPickerIdentity::HostedCodex(target)
            | SessionPickerIdentity::HostedProvider(target)
            | SessionPickerIdentity::NativeClaude(target) => {
                String::from(target.session_id.clone())
            }
        }
    }
}

#[cfg(test)]
#[path = "session_action_selection_tests.rs"]
mod tests;
