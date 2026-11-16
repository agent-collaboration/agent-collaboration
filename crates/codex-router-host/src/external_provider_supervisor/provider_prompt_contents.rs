//! Provider-neutral prompt admission and API task-input projection.

use super::{
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ConversationOperationSettlement,
    ConversationOperationSubmission, ConversationPromptRequest, ExternalProviderSupervisor,
    OperationId, PreparedOperation, ProviderConversationFuture, ProviderIdentity,
    ProviderOperationCompletion, ProviderOperationEffect, ProviderOperationKind, SessionRef,
    admitted_submission, existing_submission, failure, optional_message_text,
    prompt_runtime_failure, retain_operation,
};

/// What a prompt to a session that is in a turn does (D1 R12).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptBusyPolicy {
    /// Refuse it: mail, wakes, scheduled runs and the ACP content commands
    /// decide steer or queue themselves, and cache warm sends only to an
    /// idle session.
    RefuseBusy,
    /// Steer it into the running turn, or queue it when the agent can't
    /// steer: the API prompt (CLI, MCP tools, API).
    SteerOrQueue,
}

/// Host-internal prompt admission. Public API requests keep their prompt
/// text as task input before reaching this boundary.
#[derive(Clone, Debug)]
pub struct ProviderPromptContentsRequest {
    pub operation_id: OperationId,
    pub input_id: session_event_model::InputId,
    pub target: SessionRef,
    pub requested_by: ProviderIdentity,
    pub approver: ProviderIdentity,
    pub input: acp_client_runtime::ProviderTurnInput,
    pub busy: PromptBusyPolicy,
}

impl ProviderPromptContentsRequest {
    pub(crate) fn from_prepared_push(
        operation_id: OperationId,
        input_id: session_event_model::InputId,
        target: SessionRef,
        requested_by: ProviderIdentity,
        approver: ProviderIdentity,
        line: &collaboration_protocol::MessageText,
    ) -> Result<Self, collaboration_contracts::DeliveryContractError> {
        let content = session_event_model::PromptContent::text(line.as_str().to_owned())
            .map_err(|_| collaboration_contracts::DeliveryContractError::ClientOperation)?;
        Ok(Self {
            operation_id,
            input_id,
            target,
            requested_by,
            approver,
            input: acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![content]),
            ),
            busy: PromptBusyPolicy::RefuseBusy,
        })
    }

    pub(super) fn from_prompt_request(
        request: ConversationPromptRequest,
    ) -> Result<Self, Box<ConversationOperationFailure>> {
        let prompt_text = match &request.prompt {
            collaboration_protocol::MessageContent::Agent { text, .. }
            | collaboration_protocol::MessageContent::HumanUser { text }
            | collaboration_protocol::MessageContent::Router { text } => text.as_str().to_owned(),
        };
        let content = session_event_model::PromptContent::text(prompt_text).map_err(|_| {
            Box::new(failure(
                ConversationOperationFailureKind::InvalidRequest,
                ConversationOperationFailureStage::Validation,
                ProviderOperationEffect::None,
                "provider prompt text is invalid",
                request.operation_id.clone(),
                Some(request.target.clone()),
            ))
        })?;
        Ok(Self {
            operation_id: request.operation_id,
            input_id: request
                .input_id
                .unwrap_or_else(session_event_model::InputId::generate),
            target: request.target,
            requested_by: request.requested_by,
            approver: request.approver,
            input: acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![content]),
            ),
            busy: PromptBusyPolicy::SteerOrQueue,
        })
    }
}

impl ExternalProviderSupervisor {
    pub fn prompt_contents(
        &self,
        request: ProviderPromptContentsRequest,
    ) -> ProviderConversationFuture<'static, ConversationOperationSubmission> {
        if request.busy == PromptBusyPolicy::SteerOrQueue {
            return self.send_api_prompt(request);
        }
        let operation_id = request.operation_id.clone();
        let failure_target = Some(request.target.clone());
        let backend = self.clone();
        retain_operation(operation_id, failure_target, async move {
            let target = request.target.clone();
            let (binding, runtime) = backend.runtime_binding(
                &target.endpoint,
                &request.operation_id,
                Some(target.clone()),
            )?;
            let prepared = backend
                .prepare_operation(
                    request.operation_id.clone(),
                    ProviderOperationKind::ConversationPrompt,
                    binding.clone(),
                    Some(&target),
                )
                .await?;
            let operation_id = request.operation_id.clone();
            let failure_target = request.target.clone();
            match prepared {
                PreparedOperation::Existing(operation) => Ok(existing_submission(operation)),
                PreparedOperation::Admitted { snapshot, live } => match backend
                    .dispatch_admitted_prompt(request, &binding, runtime, snapshot, live)
                    .await
                {
                    AdmittedPromptDispatch::Done(result) => *result,
                    // Only SteerOrQueue hands a running turn back; RefuseBusy
                    // settles it as the Busy refusal it is.
                    AdmittedPromptDispatch::TurnRunning { live, .. } => {
                        let refusal = dispatch_refusal_failure(
                            acp_client_runtime::ProviderDispatchRefusal::Busy,
                            operation_id.clone(),
                            failure_target,
                        );
                        let settled = refusal.clone();
                        backend.spawn_operation(operation_id, live, async move {
                            ProviderOperationCompletion::Failure(settled)
                        });
                        Err(refusal)
                    }
                },
            }
        })
    }

    /// Sends an admitted prompt and waits for the dispatch observation; the
    /// turn's completion settles the operation in the background. A Busy
    /// refusal under SteerOrQueue leaves the operation admitted and unsettled,
    /// so the caller can steer or queue it under the same ID (D1 R12).
    pub(super) async fn dispatch_admitted_prompt(
        &self,
        request: ProviderPromptContentsRequest,
        binding: &super::ProviderBindingIdentity,
        runtime: std::sync::Arc<crate::ExternalProviderRuntime>,
        snapshot: super::ConversationOperationSnapshot,
        live: std::sync::Arc<super::LiveOperation>,
    ) -> AdmittedPromptDispatch {
        let target = request.target.clone();
        let approval_context = crate::ExternalProviderApprovalContext {
            requester: request.requested_by.clone(),
            approver: request.approver.clone(),
            target: target.clone(),
            operation_id: request.operation_id.clone(),
            binding_generation: binding.generation.clone(),
            binding_retirement: runtime.retirement(),
        };
        self.remember_turn_approver(&request.operation_id, &request.approver);
        let (dispatch, dispatched) = tokio::sync::oneshot::channel();
        let operation_id = request.operation_id.clone();
        let provider_session_id = String::from(target.session_id.clone());
        let input_id = request.input_id;
        let submission_input = request.input.clone();
        // The submission runs on its own task; only a dispatched prompt hands
        // its result to the operation, so a Busy refusal can be re-decided.
        let submitted = tokio::spawn(async move {
            runtime
                .submit_with_approval_dispatch_for_input(
                    provider_session_id,
                    input_id,
                    submission_input,
                    approval_context,
                    Some(dispatch),
                )
                .await
        });
        let completion_operation_id = operation_id.clone();
        let completion_target = target.clone();
        let complete = move |submitted: tokio::task::JoinHandle<_>| async move {
            match submitted.await {
                Ok(result) => prompt_completion(result, completion_operation_id, completion_target),
                Err(_) => ProviderOperationCompletion::Failure(failure(
                    ConversationOperationFailureKind::OutcomeUnknown,
                    ConversationOperationFailureStage::Dispatch,
                    ProviderOperationEffect::Unknown,
                    "provider dispatch outcome is unknown",
                    operation_id,
                    None,
                )),
            }
        };
        let observation =
            tokio::time::timeout(std::time::Duration::from_secs(10), dispatched).await;
        let result = match observation {
            Ok(Ok(acp_client_runtime::ProviderPromptDispatchObservation::Submitted)) => {
                Ok(admitted_submission(snapshot))
            }
            Ok(Ok(acp_client_runtime::ProviderPromptDispatchObservation::Refused(
                acp_client_runtime::ProviderDispatchRefusal::Busy,
            ))) if request.busy == PromptBusyPolicy::SteerOrQueue => {
                self.forget_turn_approver(&request.operation_id);
                // Nothing was sent: the row waits again rather than stay Unknown.
                if let Err(undo) = self
                    .undo_dispatch_mark(&request.operation_id, &target)
                    .await
                {
                    self.spawn_operation(request.operation_id, live, complete(submitted));
                    return AdmittedPromptDispatch::Done(Box::new(Err(undo)));
                }
                return AdmittedPromptDispatch::TurnRunning { live };
            }
            Ok(Ok(acp_client_runtime::ProviderPromptDispatchObservation::Refused(reason))) => Err(
                dispatch_refusal_failure(reason, request.operation_id.clone(), target.clone()),
            ),
            Ok(Ok(acp_client_runtime::ProviderPromptDispatchObservation::NotSubmitted)) => {
                Err(failure(
                    ConversationOperationFailureKind::Unavailable,
                    ConversationOperationFailureStage::Dispatch,
                    ProviderOperationEffect::None,
                    "provider input was not submitted",
                    request.operation_id.clone(),
                    Some(target.clone()),
                ))
            }
            Ok(Err(_)) | Err(_) => Err(failure(
                ConversationOperationFailureKind::OutcomeUnknown,
                ConversationOperationFailureStage::Dispatch,
                ProviderOperationEffect::Unknown,
                "provider dispatch outcome is unknown",
                request.operation_id.clone(),
                Some(target),
            )),
        };
        self.spawn_operation(request.operation_id, live, complete(submitted));
        AdmittedPromptDispatch::Done(Box::new(result))
    }
}

/// What sending an admitted prompt came to.
pub(super) enum AdmittedPromptDispatch {
    /// Dispatched, refused or uncertain: the operation is settled as before.
    Done(Box<Result<ConversationOperationSubmission, ConversationOperationFailure>>),
    /// A turn was running (SteerOrQueue only): nothing was sent, and the
    /// operation is still admitted and unsettled.
    TurnRunning {
        live: std::sync::Arc<super::LiveOperation>,
    },
}

/// The operation's completion from a dispatched prompt's turn.
pub(super) fn prompt_completion(
    result: Result<crate::ExternalProviderPromptOutcome, crate::ExternalProviderRuntimeError>,
    operation_id: OperationId,
    target: SessionRef,
) -> ProviderOperationCompletion {
    match result {
        Ok(crate::ExternalProviderPromptOutcome {
            permission_refusal_reason: Some(reason),
            ..
        }) => ProviderOperationCompletion::Failure(failure(
            ConversationOperationFailureKind::PermissionRejected,
            ConversationOperationFailureStage::Settlement,
            ProviderOperationEffect::Applied,
            reason.code(),
            operation_id,
            Some(target),
        )),
        Ok(outcome) => match optional_message_text(outcome.output) {
            Ok(response) => ProviderOperationCompletion::Success {
                settlement: ConversationOperationSettlement::PromptCompleted {
                    target: target.clone(),
                    stop_reason: outcome.stop_reason,
                    response,
                },
                target: Some(target),
                session_record: None,
            },
            Err(_) => ProviderOperationCompletion::Failure(failure(
                ConversationOperationFailureKind::ProtocolViolation,
                ConversationOperationFailureStage::Settlement,
                ProviderOperationEffect::Applied,
                "provider returned invalid prompt output",
                operation_id,
                Some(target),
            )),
        },
        Err(error) => ProviderOperationCompletion::Failure(prompt_runtime_failure(
            operation_id,
            Some(target),
            error,
        )),
    }
}

/// The failure a refused dispatch reports; nothing reached the agent.
pub(super) fn dispatch_refusal_failure(
    reason: acp_client_runtime::ProviderDispatchRefusal,
    operation_id: OperationId,
    target: SessionRef,
) -> ConversationOperationFailure {
    use acp_client_runtime::{ProviderDispatchRefusal, ProviderError};
    let (kind, message) = match reason {
        ProviderDispatchRefusal::Behavior(ProviderError::InvalidSelection) => (
            ConversationOperationFailureKind::InvalidRequest,
            "invalid skill selection",
        ),
        ProviderDispatchRefusal::Behavior(
            ProviderError::Unsupported | ProviderError::SourceUnavailable,
        ) => (
            ConversationOperationFailureKind::UnsupportedCapability,
            "provider input is unsupported",
        ),
        ProviderDispatchRefusal::Behavior(ProviderError::MalformedMetadata) => (
            ConversationOperationFailureKind::InvalidRequest,
            "provider metadata is malformed",
        ),
        ProviderDispatchRefusal::Busy => (
            ConversationOperationFailureKind::Busy,
            "provider session is busy",
        ),
        ProviderDispatchRefusal::SettingsUnresolved => (
            ConversationOperationFailureKind::SettingsUnresolved,
            "provider settings are unresolved",
        ),
        ProviderDispatchRefusal::UnsupportedContent(_) => (
            ConversationOperationFailureKind::UnsupportedCapability,
            "provider content is unsupported",
        ),
    };
    failure(
        kind,
        ConversationOperationFailureStage::Dispatch,
        ProviderOperationEffect::None,
        message,
        operation_id,
        Some(target),
    )
}

#[cfg(test)]
#[path = "provider_prompt_contents_test.rs"]
mod tests;
