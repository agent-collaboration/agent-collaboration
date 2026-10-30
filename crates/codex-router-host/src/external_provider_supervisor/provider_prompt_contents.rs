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
        })
    }
}

impl ExternalProviderSupervisor {
    pub fn prompt_contents(
        &self,
        request: ProviderPromptContentsRequest,
    ) -> ProviderConversationFuture<'static, ConversationOperationSubmission> {
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
            let approval_context = crate::ExternalProviderApprovalContext {
                requester: request.requested_by.clone(),
                approver: request.approver.clone(),
                target: target.clone(),
                operation_id: request.operation_id.clone(),
                binding_generation: binding.generation.clone(),
                binding_retirement: runtime.retirement(),
            };
            let prepared = backend
                .prepare_operation(
                    request.operation_id.clone(),
                    ProviderOperationKind::ConversationPrompt,
                    binding,
                    Some(&target),
                )
                .await?;
            match prepared {
                PreparedOperation::Existing(operation) => Ok(existing_submission(operation)),
                PreparedOperation::Admitted { snapshot, live } => {
                    let (dispatch, dispatched) = tokio::sync::oneshot::channel();
                    let operation_id = request.operation_id.clone();
                    let provider_session_id = String::from(target.session_id.clone());
                    let input_id = request.input_id;
                    let completion_target = target.clone();
                    backend.spawn_operation(operation_id.clone(), live, async move {
                        match runtime
                            .submit_with_approval_dispatch_for_input(
                                provider_session_id,
                                input_id,
                                request.input,
                                approval_context,
                                Some(dispatch),
                            )
                            .await
                        {
                            Ok(crate::ExternalProviderPromptOutcome {
                                permission_refusal_reason: Some(reason),
                                ..
                            }) => ProviderOperationCompletion::Failure(failure(
                                ConversationOperationFailureKind::PermissionRejected,
                                ConversationOperationFailureStage::Settlement,
                                ProviderOperationEffect::Applied,
                                reason.code(),
                                operation_id,
                                Some(completion_target),
                            )),
                            Ok(outcome) => match optional_message_text(outcome.output) {
                                Ok(response) => ProviderOperationCompletion::Success {
                                    settlement: ConversationOperationSettlement::PromptCompleted {
                                        target: completion_target.clone(),
                                        stop_reason: outcome.stop_reason,
                                        response,
                                    },
                                    target: Some(completion_target),
                                    session_record: None,
                                },
                                Err(_) => ProviderOperationCompletion::Failure(failure(
                                    ConversationOperationFailureKind::ProtocolViolation,
                                    ConversationOperationFailureStage::Settlement,
                                    ProviderOperationEffect::Applied,
                                    "provider returned invalid prompt output",
                                    operation_id,
                                    Some(completion_target),
                                )),
                            },
                            Err(error) => {
                                ProviderOperationCompletion::Failure(prompt_runtime_failure(
                                    operation_id,
                                    Some(completion_target),
                                    error,
                                ))
                            }
                        }
                    });
                    match tokio::time::timeout(std::time::Duration::from_secs(10), dispatched).await
                    {
                        Ok(Ok(
                            acp_client_runtime::ProviderPromptDispatchObservation::Submitted,
                        )) => Ok(admitted_submission(snapshot)),
                        Ok(Ok(acp_client_runtime::ProviderPromptDispatchObservation::Refused(
                            reason,
                        ))) => {
                            use acp_client_runtime::{ProviderDispatchRefusal, ProviderError};
                            let (kind, message) = match reason {
                                ProviderDispatchRefusal::Behavior(
                                    ProviderError::InvalidSelection,
                                ) => (
                                    ConversationOperationFailureKind::InvalidRequest,
                                    "invalid skill selection",
                                ),
                                ProviderDispatchRefusal::Behavior(
                                    ProviderError::Unsupported | ProviderError::SourceUnavailable,
                                ) => (
                                    ConversationOperationFailureKind::UnsupportedCapability,
                                    "provider input is unsupported",
                                ),
                                ProviderDispatchRefusal::Behavior(
                                    ProviderError::MalformedMetadata,
                                ) => (
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
                            Err(failure(
                                kind,
                                ConversationOperationFailureStage::Dispatch,
                                ProviderOperationEffect::None,
                                message,
                                request.operation_id,
                                Some(target),
                            ))
                        }
                        Ok(Ok(
                            acp_client_runtime::ProviderPromptDispatchObservation::NotSubmitted,
                        )) => Err(failure(
                            ConversationOperationFailureKind::Unavailable,
                            ConversationOperationFailureStage::Dispatch,
                            ProviderOperationEffect::None,
                            "provider input was not submitted",
                            request.operation_id,
                            Some(target),
                        )),
                        Ok(Err(_)) | Err(_) => Err(failure(
                            ConversationOperationFailureKind::OutcomeUnknown,
                            ConversationOperationFailureStage::Dispatch,
                            ProviderOperationEffect::Unknown,
                            "provider dispatch outcome is unknown",
                            request.operation_id,
                            Some(target),
                        )),
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderPromptContentsRequest;
    use collaboration_protocol::{
        ConversationPromptRequest, MessageContent, MessageText, OperationId, SessionRef,
    };
    use session_event_model::PromptContent;

    #[test]
    fn prompt_request_forwards_exact_task_text_without_router_envelope() {
        let target: SessionRef = serde_json::from_value(serde_json::json!({
            "endpoint":{"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"codex-local"},
            "sessionId":"target-session"
        }))
        .expect("target session");
        let sender: SessionRef = serde_json::from_value(serde_json::json!({
            "endpoint":{"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"claude-local"},
            "sessionId":"sender-session"
        }))
        .expect("sender session");
        let request = ConversationPromptRequest {
            operation_id: OperationId::generate(),
            input_id: None,
            target,
            generation: None,
            requested_by: sender.clone().into(),
            approver: sender.clone().into(),
            prompt: MessageContent::Agent {
                sender,
                text: MessageText::try_from("Check the provider prompt.".to_owned())
                    .expect("prompt text"),
            },
        };

        let provider_request = ProviderPromptContentsRequest::from_prompt_request(request)
            .expect("provider prompt contents");
        let acp_client_runtime::ProviderTurnInput::Prompt(submission) = provider_request.input
        else {
            panic!("ordinary prompt input");
        };
        let Some(PromptContent::Text { text }) = submission.content.first() else {
            panic!("one text prompt should be generated");
        };
        assert_eq!(text.as_str(), "Check the provider prompt.");
        assert!(!text.as_str().contains("Agent communication"));
        assert!(!text.as_str().contains("Self-declared sender:"));
    }
}
