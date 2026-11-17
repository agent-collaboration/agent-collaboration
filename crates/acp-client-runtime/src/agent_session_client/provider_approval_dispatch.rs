//! Approval context lifetime for a dispatched provider prompt.

use super::approval_turn_cancellation::PermissionResponseBarrier;
use super::{
    ActiveApprovalContext, AgentSessionClient, ApprovalContextGuard, ExternalProviderPromptOutcome,
    ExternalProviderRuntimeError, ProviderPromptDispatchObservation,
};
use session_event_model::InputId;
use std::sync::Arc;

/// Ends a prompt's turn on its session's cell when dropped. The Host drops
/// it once the prompt's operation has settled (design note §13.1).
pub struct ProviderTurnEnd(super::session_cell_bookkeeping::CellTurnGuard);

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
        .0
    }

    /// Sends a prompt as the session's turn (a queued one under `queued_turn`)
    /// and hands back the turn's end: the caller drops it once the prompt's
    /// operation has settled, so the line commits nothing before that
    /// (design note §13.1).
    pub async fn submit_holding_turn_end(
        &self,
        provider_session_id: String,
        input_id: InputId,
        input: crate::ProviderTurnInput,
        context: P::Context,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
        queued_turn: Option<crate::QueueTurn>,
    ) -> (
        Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError>,
        Option<ProviderTurnEnd>,
    ) {
        self.submit_turn(
            provider_session_id,
            input_id,
            input,
            context,
            dispatch,
            queued_turn,
        )
        .await
    }

    /// One prompt as the session's one turn (design note §3.5). A direct
    /// prompt opens the turn on the session's cell first; a queued one runs
    /// under the turn its handover opened. Returns the turn's end, which
    /// ends the turn when dropped, after the prompt's approval context is
    /// released; a refused direct prompt opened none.
    async fn submit_turn(
        &self,
        provider_session_id: String,
        input_id: InputId,
        input: crate::ProviderTurnInput,
        context: P::Context,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
        queued_turn: Option<crate::QueueTurn>,
    ) -> (
        Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError>,
        Option<ProviderTurnEnd>,
    ) {
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
                    return (Err(refusal.error), None);
                }
                turn
            }
        };
        let mut turn_end = ProviderTurnEnd(super::session_cell_bookkeeping::CellTurnGuard::new(
            &self.session_cells,
            &provider_session_id,
            turn,
        ));
        let result = self
            .submit_in_turn(
                provider_session_id,
                input_id,
                input,
                context,
                dispatch,
                &mut turn_end,
            )
            .await;
        (result, Some(turn_end))
    }

    /// The prompt itself, inside its turn; its approval context is released
    /// before this returns.
    async fn submit_in_turn(
        &self,
        provider_session_id: String,
        input_id: InputId,
        input: crate::ProviderTurnInput,
        context: P::Context,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
        turn_end: &mut ProviderTurnEnd,
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
                turn_end.0.not_sent();
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
        turn_end.0.applied();
        outcome.permission_refusal_reason = self
            .permission_refusal_reasons
            .lock()
            .ok()
            .and_then(|mut refusals| refusals.remove(&operation_id));
        Ok(outcome)
    }
}
