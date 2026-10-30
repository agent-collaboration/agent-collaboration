use acp_client_runtime::{
    PreparedCompact, ProviderBehavior, ProviderError, ProviderFacts, ProviderUpdate,
};
use session_event_model::{PromptContent, ProviderSubmission, SkillSourceError, SkillView};

/// Explicit direct-runtime control; configured Host providers use their own behavior.
pub struct StandardAcpBehavior;

impl ProviderBehavior for StandardAcpBehavior {
    fn observe(&self, update: ProviderUpdate) -> Result<ProviderFacts, ProviderError> {
        let skill_view = match update {
            ProviderUpdate::SessionAdmitted | ProviderUpdate::Commands { .. } => Some(
                SkillView::Unavailable(SkillSourceError::UnestablishedClassification),
            ),
            ProviderUpdate::SessionClosed => Some(SkillView::Unknown),
            ProviderUpdate::ToolStatus { .. } => None,
        };
        Ok(ProviderFacts {
            skill_view,
            compaction: None,
        })
    }

    fn prepare(&self, submission: ProviderSubmission) -> Result<Vec<PromptContent>, ProviderError> {
        if !submission.selected.is_empty() {
            return Err(ProviderError::SourceUnavailable);
        }
        Ok(submission.content)
    }

    fn compact(&self) -> Result<PreparedCompact, ProviderError> {
        Err(ProviderError::Unsupported)
    }
}
