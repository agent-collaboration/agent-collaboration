use acp_client_runtime::{
    PreparedCompact, ProviderBehavior, ProviderError, ProviderFacts, ProviderUpdate,
};
use session_event_model::{PromptContent, ProviderSubmission};

pub(crate) struct TestProviderBehavior;
impl ProviderBehavior for TestProviderBehavior {
    fn can_reuse_setting_confirmation(
        &self,
        _kind: acp_client_runtime::ProviderSettingKind,
    ) -> bool {
        true
    }
    fn model_metadata_source(&self) -> acp_client_runtime::ModelMetadataSource {
        acp_client_runtime::ModelMetadataSource::Unavailable
    }
    fn decode_model_metadata(
        &self,
        _value: &serde_json::Value,
    ) -> acp_client_runtime::ModelReasoningMetadata {
        acp_client_runtime::ModelReasoningMetadata::Unknown(
            acp_client_runtime::ModelReasoningUnknown::UnsupportedSource,
        )
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
