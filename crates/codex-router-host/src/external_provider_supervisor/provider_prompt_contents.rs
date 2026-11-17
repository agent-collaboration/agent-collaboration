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
    /// The API prompt (CLI, MCP tools, API): the caller's delivery choice
    /// decides. Auto steers if the agent can and otherwise refuses with
    /// CannotSteer; queue waits in the session's line; steer steers or is
    /// refused.
    Send(collaboration_protocol::MessageDelivery),
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
            busy: PromptBusyPolicy::Send(request.delivery),
        })
    }
}

impl ExternalProviderSupervisor {
    pub fn prompt_contents(
        &self,
        request: ProviderPromptContentsRequest,
    ) -> ProviderConversationFuture<'static, ConversationOperationSubmission> {
        if let PromptBusyPolicy::Send(delivery) = request.busy {
            return self.send_api_prompt(request, delivery);
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
                    // Only Send hands a running turn back; RefuseBusy
                    // settles it as the Busy refusal it is.
                    AdmittedPromptDispatch::TurnRunning { live, refusal } => {
                        let refusal =
                            dispatch_refusal_failure(refusal, operation_id.clone(), failure_target);
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
    /// refusal under Send leaves the operation admitted and unsettled,
    /// so the caller can steer or queue it under the same ID (D1 R12).
    pub(super) async fn dispatch_admitted_prompt(
        &self,
        request: ProviderPromptContentsRequest,
        binding: &super::ProviderBindingIdentity,
        runtime: std::sync::Arc<crate::ExternalProviderRuntime>,
        snapshot: super::ConversationOperationSnapshot,
        live: std::sync::Arc<super::LiveOperation>,
    ) -> AdmittedPromptDispatch {
        let started = self.start_admitted_submission(&request, binding, runtime, None);
        self.observe_admitted_submission(request, started, snapshot, live)
            .await
    }

    /// The submission boundary: spawns the provider submission, which may
    /// reach the agent from here on. It never awaits, so a check made right
    /// before it is the last word before the submission leaves (D1 R14).
    pub(super) fn start_admitted_submission(
        &self,
        request: &ProviderPromptContentsRequest,
        binding: &super::ProviderBindingIdentity,
        runtime: std::sync::Arc<crate::ExternalProviderRuntime>,
        queue_turn: Option<acp_client_runtime::QueueTurn>,
    ) -> StartedSubmission {
        let target = request.target.clone();
        let approval_context = crate::ExternalProviderApprovalContext {
            requester: request.requested_by.clone(),
            approver: request.approver.clone(),
            target: target.clone(),
            operation_id: request.operation_id.clone(),
            binding_generation: binding.generation.clone(),
            binding_retirement: runtime.retirement(),
        };
        let (dispatch, dispatched) = tokio::sync::oneshot::channel();
        let provider_session_id = String::from(target.session_id);
        let input_id = request.input_id.clone();
        let submission_input = request.input.clone();
        // The submission runs on its own task; only a dispatched prompt hands
        // its result to the operation, so a Busy refusal can be re-decided.
        // A queued message runs under the turn its handover opened.
        let submitted = tokio::spawn(async move {
            runtime
                .submit_holding_turn_end(
                    provider_session_id,
                    input_id,
                    submission_input,
                    approval_context,
                    Some(dispatch),
                    queue_turn,
                )
                .await
        });
        StartedSubmission {
            submitted,
            dispatched,
        }
    }

    /// Waits for a started submission's dispatch observation; the turn's
    /// completion settles the operation in the background.
    pub(super) async fn observe_admitted_submission(
        &self,
        request: ProviderPromptContentsRequest,
        started: StartedSubmission,
        snapshot: super::ConversationOperationSnapshot,
        live: std::sync::Arc<super::LiveOperation>,
    ) -> AdmittedPromptDispatch {
        let StartedSubmission {
            submitted,
            dispatched,
        } = started;
        let target = request.target.clone();
        let operation_id = request.operation_id.clone();
        let completion_operation_id = operation_id.clone();
        let completion_target = target.clone();
        let complete = move |submitted: tokio::task::JoinHandle<_>| async move {
            match submitted.await {
                Ok((result, turn_end)) => (
                    prompt_completion(result, completion_operation_id, completion_target),
                    turn_end,
                ),
                Err(_) => (
                    ProviderOperationCompletion::Failure(failure(
                        ConversationOperationFailureKind::OutcomeUnknown,
                        ConversationOperationFailureStage::Dispatch,
                        ProviderOperationEffect::Unknown,
                        "provider dispatch outcome is unknown",
                        operation_id,
                        None,
                    )),
                    None,
                ),
            }
        };
        #[cfg(test)]
        {
            let pause = self
                .inner
                .observation_test_pause
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(pause) = pause {
                let _entered = pause.entered.send(());
                let _released = pause.release.await;
            }
        }
        let observation =
            tokio::time::timeout(std::time::Duration::from_secs(10), dispatched).await;
        let result = match observation {
            Ok(Ok(acp_client_runtime::ProviderPromptDispatchObservation::Submitted)) => {
                Ok(admitted_submission(snapshot))
            }
            Ok(Ok(acp_client_runtime::ProviderPromptDispatchObservation::Refused(
                refusal @ acp_client_runtime::ProviderDispatchRefusal::Busy { .. },
            ))) if matches!(request.busy, PromptBusyPolicy::Send(_)) => {
                // Nothing was sent: the row waits again rather than stay Unknown.
                if let Err(undo) = self
                    .undo_dispatch_mark(&request.operation_id, &target)
                    .await
                {
                    self.spawn_prompt_operation(request.operation_id, live, complete(submitted));
                    return AdmittedPromptDispatch::Done(Box::new(Err(undo)));
                }
                return AdmittedPromptDispatch::TurnRunning { live, refusal };
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
        self.spawn_prompt_operation(request.operation_id, live, complete(submitted));
        AdmittedPromptDispatch::Done(Box::new(result))
    }
}

/// A provider submission already spawned: it may have reached the agent.
pub(super) struct StartedSubmission {
    submitted: tokio::task::JoinHandle<(
        Result<crate::ExternalProviderPromptOutcome, crate::ExternalProviderRuntimeError>,
        Option<acp_client_runtime::ProviderTurnEnd>,
    )>,
    dispatched:
        tokio::sync::oneshot::Receiver<acp_client_runtime::ProviderPromptDispatchObservation>,
}

/// What sending an admitted prompt came to.
pub(super) enum AdmittedPromptDispatch {
    /// Dispatched, refused or uncertain: the operation is settled as before.
    Done(Box<Result<ConversationOperationSubmission, ConversationOperationFailure>>),
    /// A turn was running (Send only): nothing was sent, and the
    /// operation is still admitted and unsettled.
    TurnRunning {
        live: std::sync::Arc<super::LiveOperation>,
        /// The agent's busy refusal, with its answer (D1 M8).
        refusal: acp_client_runtime::ProviderDispatchRefusal,
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

/// Why a refused dispatch never reached the agent, as its failure says it.
pub(crate) fn dispatch_refusal_reason(
    reason: acp_client_runtime::ProviderDispatchRefusal,
    operation_id: OperationId,
    target: SessionRef,
) -> String {
    String::from(dispatch_refusal_failure(reason, operation_id, target).message)
}

/// The failure a refused dispatch reports; nothing reached the agent.
pub(super) fn dispatch_refusal_failure(
    reason: acp_client_runtime::ProviderDispatchRefusal,
    operation_id: OperationId,
    target: SessionRef,
) -> ConversationOperationFailure {
    use acp_client_runtime::{ProviderDispatchRefusal, ProviderError};
    let session_answer;
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
        // Session refusals carry their answer: the state and the valid next
        // steps (D1 M8). The kinds are unchanged.
        ProviderDispatchRefusal::Busy { answer } => {
            session_answer = answer;
            (
                ConversationOperationFailureKind::Busy,
                session_answer.as_str(),
            )
        }
        ProviderDispatchRefusal::SettingsUnresolved { answer } => {
            session_answer = answer;
            (
                ConversationOperationFailureKind::SettingsUnresolved,
                session_answer.as_str(),
            )
        }
        // Nothing was submitted, as before; the answer says why.
        ProviderDispatchRefusal::NotLoaded { answer } => {
            session_answer = answer;
            (
                ConversationOperationFailureKind::Unavailable,
                session_answer.as_str(),
            )
        }
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
