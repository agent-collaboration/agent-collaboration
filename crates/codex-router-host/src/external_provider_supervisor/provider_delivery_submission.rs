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
        self.spawn_operation(operation_id, live, async move {
            let result = runtime
                .submit_with_approval_dispatch_for_input(
                    provider_session_id,
                    input_id,
                    request.input,
                    approval_context,
                    Some(dispatched),
                )
                .await;
            // Same settlement as an unqueued prompt, permission refusal
            // included (D1 C5): mail's refusals were recorded as completed.
            super::provider_prompt_contents::prompt_completion(
                result,
                completion_operation_id,
                completion_target,
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
