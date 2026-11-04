//! Approval context lifetime for a dispatched provider prompt.

use super::approval_turn_cancellation::PermissionResponseBarrier;
use super::{
    ActiveApprovalContext, AgentSessionClient, ApprovalContextGuard, ExternalProviderPromptOutcome,
    ExternalProviderRuntimeError, ProviderPromptDispatchObservation,
};
use session_event_model::InputId;
use std::sync::Arc;

impl<P: crate::InteractionPort> AgentSessionClient<P> {
    /// Preserve the typed approval context for a multi-block delivery turn.
    pub async fn submit_with_approval_dispatch_for_input(
        &self,
        provider_session_id: String,
        input_id: InputId,
        input: crate::ProviderTurnInput,
        context: P::Context,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        let operation_id = P::operation_id(&context);
        let binding_retirement = P::binding_retirement(&context);
        {
            let mut contexts = self.approval_contexts.lock().map_err(|_| {
                ExternalProviderRuntimeError::Operation(
                    "provider approval context unavailable".to_owned(),
                )
            })?;
            if contexts.contains_key(&provider_session_id) {
                if let Some(dispatch) = dispatch {
                    let _result = dispatch.send(ProviderPromptDispatchObservation::Refused(
                        crate::ProviderDispatchRefusal::Busy,
                    ));
                }
                return Err(ExternalProviderRuntimeError::LocalBusy);
            }
            contexts.insert(
                provider_session_id.clone(),
                ActiveApprovalContext {
                    approval: context,
                    cancelling: tokio_util::sync::CancellationToken::new(),
                    responses: Arc::new(PermissionResponseBarrier::default()),
                },
            );
        }
        let _context_guard = ApprovalContextGuard {
            contexts: Arc::clone(&self.approval_contexts),
            provider_session_id: provider_session_id.clone(),
            operation_id: operation_id.clone(),
        };
        let prompt_result = self
            .prompt_content(
                provider_session_id,
                input_id,
                Some(operation_id.clone()),
                input,
                dispatch,
            )
            .await;
        if binding_retirement.is_cancelled() {
            return Err(ExternalProviderRuntimeError::TransportFailure);
        }
        let mut outcome = prompt_result?;
        outcome.permission_refusal_reason = self
            .permission_refusal_reasons
            .lock()
            .ok()
            .and_then(|mut refusals| refusals.remove(&operation_id));
        Ok(outcome)
    }
}
