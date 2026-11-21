//! Provider-neutral prompt admission and API task-input projection.

use super::{
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ConversationOperationSubmission, ConversationPromptRequest,
    ExternalProviderSupervisor, OperationId, ProviderConversationFuture, ProviderIdentity,
    ProviderOperationEffect, SessionRef, failure,
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
        self.owner_prompt(request)
    }
}
