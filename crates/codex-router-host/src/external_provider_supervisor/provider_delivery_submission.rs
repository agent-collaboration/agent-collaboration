//! Report owner submission to the scheduled-run delivery adapter.

use super::{
    ConversationOperationFailure, ExternalProviderSupervisor, ProviderPromptContentsRequest,
};
use crate::session_owner_calls::InputAcceptance;
use provider_session_machines::session_owner::RevokeReason;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProviderPromptDispatch {
    Submitted,
    NotSubmitted { reason: Option<RevokeReason> },
    Uncertain,
}

impl ExternalProviderSupervisor {
    pub(crate) async fn submit_delivery_prompt_contents(
        &self,
        request: ProviderPromptContentsRequest,
    ) -> Result<ProviderPromptDispatch, Box<ConversationOperationFailure>> {
        self.submit_owned_direct_delivery(request)
            .await
            .map(|acceptance| match acceptance {
                InputAcceptance::Started => ProviderPromptDispatch::Submitted,
                InputAcceptance::Unsent { reason } => {
                    ProviderPromptDispatch::NotSubmitted { reason }
                }
                _ => ProviderPromptDispatch::Uncertain,
            })
            .map_err(Box::new)
    }
}
