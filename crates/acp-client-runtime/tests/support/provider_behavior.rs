use acp_client_runtime::{
    PreparedCompact, ProviderBehavior, ProviderError, ProviderFacts, ProviderUpdate,
};
use session_event_model::{PromptContent, ProviderSubmission};

pub(crate) struct TestProviderBehavior;
impl ProviderBehavior for TestProviderBehavior {
    fn initialization_authentication(
        &self,
    ) -> acp_client_runtime::ProviderInitializationAuthentication {
        acp_client_runtime::ProviderInitializationAuthentication::NotRequested
    }
    fn can_reuse_setting_confirmation(
        &self,
        _kind: provider_session_machines::ProviderSettingKind,
    ) -> bool {
        true
    }
    fn model_metadata_source(&self) -> acp_client_runtime::ModelMetadataSource {
        acp_client_runtime::ModelMetadataSource::Unavailable
    }
    fn decode_model_metadata(
        &self,
        _value: &serde_json::Value,
    ) -> provider_session_machines::ModelReasoningMetadata {
        provider_session_machines::ModelReasoningMetadata::Unknown(
            provider_session_machines::ModelReasoningUnknown::UnsupportedSource,
        )
    }
    fn auto_permission_mode(&self) -> acp_client_runtime::AutoPermissionMode {
        acp_client_runtime::AutoPermissionMode::UNREALIZED
    }
    fn observe(&self, _: ProviderUpdate) -> Result<ProviderFacts, ProviderError> {
        Ok(ProviderFacts::default())
    }
    fn prepare(&self, submission: ProviderSubmission) -> Result<Vec<PromptContent>, ProviderError> {
        if !submission.selected.is_empty() {
            return Err(ProviderError::Unsupported);
        }
        Ok(submission.content)
    }
    fn compact(&self) -> Result<PreparedCompact, ProviderError> {
        Err(ProviderError::Unsupported)
    }
}

/// Claude's declaration of auto: its own `auto`, with `acceptEdits` as the
/// fallback it may land on. Everything else is the plain test behaviour.
pub(crate) struct ClaudeLikeAutoBehavior;
impl ProviderBehavior for ClaudeLikeAutoBehavior {
    fn initialization_authentication(
        &self,
    ) -> acp_client_runtime::ProviderInitializationAuthentication {
        TestProviderBehavior.initialization_authentication()
    }
    fn can_reuse_setting_confirmation(
        &self,
        kind: provider_session_machines::ProviderSettingKind,
    ) -> bool {
        TestProviderBehavior.can_reuse_setting_confirmation(kind)
    }
    fn model_metadata_source(&self) -> acp_client_runtime::ModelMetadataSource {
        TestProviderBehavior.model_metadata_source()
    }
    fn decode_model_metadata(
        &self,
        value: &serde_json::Value,
    ) -> provider_session_machines::ModelReasoningMetadata {
        TestProviderBehavior.decode_model_metadata(value)
    }
    fn auto_permission_mode(&self) -> acp_client_runtime::AutoPermissionMode {
        acp_client_runtime::AutoPermissionMode {
            realized_by: Some("auto"),
            fallback: Some("acceptEdits"),
            realization: acp_client_runtime::AutoRealization::Unknown,
            stand_in_reason: None,
        }
    }
    fn observe(&self, update: ProviderUpdate) -> Result<ProviderFacts, ProviderError> {
        TestProviderBehavior.observe(update)
    }
    fn prepare(&self, submission: ProviderSubmission) -> Result<Vec<PromptContent>, ProviderError> {
        TestProviderBehavior.prepare(submission)
    }
    fn compact(&self) -> Result<PreparedCompact, ProviderError> {
        TestProviderBehavior.compact()
    }
}
