use acp_client_runtime::{
    PreparedCompact, ProviderBehavior, ProviderError, ProviderFacts, ProviderUpdate,
};
use session_event_model::{PromptContent, ProviderSubmission};

pub(crate) struct TestProviderBehavior;
impl ProviderBehavior for TestProviderBehavior {
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
