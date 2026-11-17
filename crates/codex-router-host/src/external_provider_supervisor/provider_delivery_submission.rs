//! Delivery prompt admission waits for the ACP request to enter the session actor.
use super::{
    ConversationOperationFailure, Duration, ExternalProviderSupervisor, PreparedOperation,
    ProviderOperationKind, ProviderPromptContentsRequest,
};
use crate::external_provider_runtime::ProviderPromptDispatchObservation;

const DELIVERY_DISPATCH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProviderPromptDispatch {
    Submitted,
    NotSubmitted,
    Existing,
    Uncertain,
    Refused(acp_client_runtime::ProviderDispatchRefusal),
}

impl ExternalProviderSupervisor {
    /// Submits caller-projected content blocks without passing through message rendering.
    pub(crate) async fn submit_delivery_prompt_contents(
        &self,
        request: ProviderPromptContentsRequest,
    ) -> Result<ProviderPromptDispatch, Box<ConversationOperationFailure>> {
        let operation_id = request.operation_id.clone();
        let target = request.target.clone();
        let (binding, runtime) = self
            .runtime_binding(&target.endpoint, &operation_id, Some(target.clone()))
            .map_err(Box::new)?;
        let approval_context = crate::ExternalProviderApprovalContext {
            requester: request.requested_by,
            approver: request.approver.clone(),
            target: target.clone(),
            operation_id: operation_id.clone(),
            binding_generation: binding.generation.clone(),
            binding_retirement: runtime.retirement(),
        };
        let prepared = self
            .prepare_operation(
                operation_id.clone(),
                ProviderOperationKind::ConversationPrompt,
                binding,
                Some(&target),
            )
            .await
            .map_err(Box::new)?;
        let PreparedOperation::Admitted { live, .. } = prepared else {
            return Ok(ProviderPromptDispatch::Existing);
        };
        // A delivery-started turn names its approver to a later steer, like an
        // API-started one; the record goes with its retained settlement (D1 C1).
        self.remember_turn_approver(&operation_id, &request.approver);
        let (dispatched, dispatch_observation) = tokio::sync::oneshot::channel();
        let input_id = request.input_id;
        let provider_session_id = String::from(target.session_id.clone());
        let completion_target = target.clone();
        let completion_operation_id = operation_id.clone();
        // The session's turn ends once this operation's settlement is
        // recorded (design note §13.1).
        self.spawn_prompt_operation(operation_id, live, async move {
            let (result, turn_end) = runtime
                .submit_holding_turn_end(
                    provider_session_id,
                    input_id,
                    request.input,
                    approval_context,
                    Some(dispatched),
                    None,
                )
                .await;
            // Same settlement as an unqueued prompt, permission refusal
            // included (D1 C5): mail's refusals were recorded as completed.
            (
                super::provider_prompt_contents::prompt_completion(
                    result,
                    completion_operation_id,
                    completion_target,
                ),
                turn_end,
            )
        });
        Ok(
            match tokio::time::timeout(DELIVERY_DISPATCH_TIMEOUT, dispatch_observation).await {
                Ok(Ok(ProviderPromptDispatchObservation::Submitted)) => {
                    ProviderPromptDispatch::Submitted
                }
                Ok(Ok(ProviderPromptDispatchObservation::NotSubmitted)) => {
                    ProviderPromptDispatch::NotSubmitted
                }
                Ok(Ok(ProviderPromptDispatchObservation::Refused(reason))) => {
                    ProviderPromptDispatch::Refused(reason)
                }
                Ok(Err(_)) | Err(_) => ProviderPromptDispatch::Uncertain,
            },
        )
    }
}

impl ExternalProviderSupervisor {
    /// Sends a queued delivery its line admitted at the head and committed:
    /// the dispatch marker, then the session's cell decides between the
    /// handover and a withdrawal (design note §12.2, §13.1). Answers as an
    /// unqueued delivery does, so the line records a refusal's reason.
    pub(crate) async fn dispatch_queued_delivery(
        &self,
        request: ProviderPromptContentsRequest,
        admitted: &super::AdmittedApiPrompt,
        queued_send: crate::external_provider_supervisor::QueuedSend,
    ) -> Result<ProviderPromptDispatch, Box<ConversationOperationFailure>> {
        let operation_id = request.operation_id.clone();
        let target = request.target.clone();
        let (binding, runtime) = self
            .runtime_binding(&target.endpoint, &operation_id, Some(target.clone()))
            .map_err(Box::new)?;
        if !admitted.claim_for_dispatch() {
            queued_send.mark_not_written(false);
            return Ok(ProviderPromptDispatch::Existing);
        }
        let live = admitted.live();
        if let Err(marker_failure) = self.begin_dispatch(&operation_id, &target).await {
            // No marker, so nothing was sent; this dispatch settles it.
            queued_send.mark_not_written(false);
            let settled = marker_failure.clone();
            self.spawn_operation(operation_id, live, async move {
                super::ProviderOperationCompletion::Failure(settled)
            });
            return Err(Box::new(marker_failure));
        }
        let turn = match queued_send.mark_written() {
            crate::external_provider_supervisor::QueuedSendStep::Send(turn) => turn,
            crate::external_provider_supervisor::QueuedSendStep::Withdraw => {
                // A close, cancel or stop won first: the marker is undone and
                // the operation settles not sent.
                let unmarked = self
                    .undo_dispatch_mark(&operation_id, &target)
                    .await
                    .is_ok();
                let reason = queued_send.unmark_done(unmarked);
                let unsent = super::failure(
                    super::ConversationOperationFailureKind::InvalidRequest,
                    super::ConversationOperationFailureStage::Dispatch,
                    super::ProviderOperationEffect::None,
                    &format!("not sent: {reason}"),
                    operation_id.clone(),
                    Some(target.clone()),
                );
                let settled = unsent.clone();
                self.spawn_operation(operation_id, live, async move {
                    super::ProviderOperationCompletion::Failure(settled)
                });
                return Err(Box::new(unsent));
            }
        };
        let approval_context = crate::ExternalProviderApprovalContext {
            requester: request.requested_by,
            approver: request.approver.clone(),
            target: target.clone(),
            operation_id: operation_id.clone(),
            binding_generation: binding.generation.clone(),
            binding_retirement: runtime.retirement(),
        };
        self.remember_turn_approver(&operation_id, &request.approver);
        let (dispatched, dispatch_observation) = tokio::sync::oneshot::channel();
        let input_id = request.input_id;
        let provider_session_id = String::from(target.session_id.clone());
        let completion_target = target.clone();
        let completion_operation_id = operation_id.clone();
        self.spawn_prompt_operation(operation_id, live, async move {
            let (result, turn_end) = runtime
                .submit_holding_turn_end(
                    provider_session_id,
                    input_id,
                    request.input,
                    approval_context,
                    Some(dispatched),
                    Some(turn),
                )
                .await;
            (
                super::provider_prompt_contents::prompt_completion(
                    result,
                    completion_operation_id,
                    completion_target,
                ),
                turn_end,
            )
        });
        Ok(
            match tokio::time::timeout(DELIVERY_DISPATCH_TIMEOUT, dispatch_observation).await {
                Ok(Ok(ProviderPromptDispatchObservation::Submitted)) => {
                    ProviderPromptDispatch::Submitted
                }
                Ok(Ok(ProviderPromptDispatchObservation::NotSubmitted)) => {
                    ProviderPromptDispatch::NotSubmitted
                }
                Ok(Ok(ProviderPromptDispatchObservation::Refused(reason))) => {
                    ProviderPromptDispatch::Refused(reason)
                }
                Ok(Err(_)) | Err(_) => ProviderPromptDispatch::Uncertain,
            },
        )
    }
}
