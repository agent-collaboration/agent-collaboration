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
        self.submit_turn(
            provider_session_id,
            input_id,
            input,
            context,
            dispatch,
            None,
        )
        .await
    }

    /// Sends a queued message's prompt under the turn its handover opened
    /// (`queue_mark_written`); the turn's end frees the line.
    pub async fn submit_queued_turn_with_approval_dispatch(
        &self,
        provider_session_id: String,
        input_id: InputId,
        input: crate::ProviderTurnInput,
        context: P::Context,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
        turn: crate::QueueTurn,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        self.submit_turn(
            provider_session_id,
            input_id,
            input,
            context,
            dispatch,
            Some(turn),
        )
        .await
    }

    /// One prompt as the session's one turn (design note §3.5). A direct
    /// prompt opens the turn on the session's cell first; a queued one runs
    /// under the turn its handover opened. The turn ends when the prompt
    /// returns, after its approval context is released, whether or not the
    /// caller is still waiting.
    async fn submit_turn(
        &self,
        provider_session_id: String,
        input_id: InputId,
        input: crate::ProviderTurnInput,
        context: P::Context,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
        queued_turn: Option<crate::QueueTurn>,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        let turn = match queued_turn {
            Some(crate::QueueTurn(turn)) => turn,
            None => {
                let turn = crate::session_turn_machine::TurnId(uuid::Uuid::now_v7());
                if let Err(refusal) = super::session_cell_bookkeeping::start_direct_turn(
                    &self.session_cells,
                    &provider_session_id,
                    turn,
                ) {
                    if let Some(dispatch) = dispatch {
                        let _result = dispatch.send(refusal.observation);
                    }
                    return Err(refusal.error);
                }
                turn
            }
        };
        // Declared before the approval context's guard, so the turn ends
        // after that context is released.
        let mut turn_guard = super::session_cell_bookkeeping::CellTurnGuard::new(
            &self.session_cells,
            &provider_session_id,
            turn,
        );
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
                turn_guard.not_sent();
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
        turn_guard.applied();
        outcome.permission_refusal_reason = self
            .permission_refusal_reasons
            .lock()
            .ok()
            .and_then(|mut refusals| refusals.remove(&operation_id));
        Ok(outcome)
    }
}
