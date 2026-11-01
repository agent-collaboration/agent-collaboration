//! Direct-message tools: send, reply, push inspection, inbox and history.
use super::{
    CallToolResult, CollaborationMcpServer, McpToolOutput, MessageReplyRequest, MessageReplyResult,
    MessageSendRequest, MessageSendResult, OperationEffect, Parameters, PushRecordHistoryParams,
    PushRecordListParams, PushRecordListResult, application_result, failure_naming,
    message_receipt_result, message_reply_receipt_result, operation_failure_result,
    rejection_failure, schemars, tool, tool_router, validation_failure,
};
use collaboration_protocol::{SessionMessageReplyParams, SessionMessageSendParams};
use collaboration_service::collaboration_application::{MessageFailure, MessageFailureKind};

#[tool_router(router = message_tool_router, vis = "pub(super)")]
impl CollaborationMcpServer {
    #[tool(name = "message_send", description = "Submits one agent-authored or explicit human message with exact auto, queue, or steer semantics. Agent sender identity is reported by the request's client, not authenticated. Supply itemId for safe same-content replay; different content under that ID is itemConflict. If omitted, the Router mints and returns an ID, and retrying an omitted-ID call after a lost local answer may duplicate. A local result includes the stored push id (the itemId), link, target and recorded delivery receipt. A remote result is queued with the item and frozen target once the sender outbox has custody; accepted input or a peer write does not prove completion or an agent reply. Router-authored content is not a public caller input, and uncertain dispatch is never replayed automatically.", output_schema = rmcp::handler::server::tool::schema_for_type::<McpToolOutput<MessageSendResult>>())]
    pub(super) async fn message_send(
        &self,
        Parameters(arguments): Parameters<UndecodedMessageSend>,
    ) -> CallToolResult {
        let arguments = serde_json::Value::Object(arguments.0);
        let request = match <MessageSendRequest as serde::Deserialize>::deserialize(&arguments) {
            Ok(request) => request,
            Err(error) => {
                // Preserve existing refusals for other fields; only the newly
                // specified identity error has the MS-2 invalidShape contract.
                let malformed_id = arguments.get("itemId").is_some_and(|id| {
                    <Option<collaboration_protocol::MailItemId> as serde::Deserialize>::deserialize(
                        id,
                    )
                    .is_err()
                });
                if !malformed_id {
                    return validation_failure(&error.to_string());
                }
                let refusal = MessageFailure {
                    kind: MessageFailureKind::InvalidShape,
                    stage: collaboration_service::collaboration_application::MessageFailureStage::Discovery,
                    message: "itemId must be a canonical UUIDv7 when supplied".into(), existing: None,
                };
                return operation_failure_result(&rejection_failure(
                    &refusal,
                    OperationEffect::None,
                ));
            }
        };
        let refusal = match &request.message {
            collaboration_client::PublicMessageContent::Agent { sender, .. } => {
                self.sender_claim_refusal(sender)
            }
            collaboration_client::PublicMessageContent::HumanUser { .. } => {
                self.operator_claim_refusal()
            }
        };
        if let Some(refusal) = refusal {
            return refusal;
        }
        let target = request.target.clone();
        let params = SessionMessageSendParams {
            item_id: request.item_id,
            target: request.target,
            message: request.message.into(),
            mode: request.delivery,
            generation_guard: request.generation_guard,
        };
        match self.application.messages().message_send(params).await {
            Ok(push) => message_receipt_result(push),
            Err(rejection) => message_failure_result(&rejection, "target", target),
        }
    }

    #[tool(name = "router_show", description = "Shows one stored Router push or the sender Outbox status of a remote message by item id. Push results include body and expanded activity; outgoing remote mail has custody status and no sender-side push. Caller identity is reported, not authenticated, and checked as a confusion guard. Showing a direct message to its target marks it read.", output_schema = rmcp::handler::server::tool::schema_for_type::<McpToolOutput<collaboration_protocol::MessageShowResult>>())]
    pub(super) async fn router_show(
        &self,
        Parameters(request): Parameters<collaboration_protocol::MessageShowRequest>,
    ) -> CallToolResult {
        application_result(
            self.application.messages().message_show(request).await,
            OperationEffect::None,
        )
    }

    #[tool(name = "message_inbox", description = "Lists retained unread direct messages for the reported caller session. Caller identity is supplied by the MCP request and is not authenticated; the service uses it as a confusion guard.", output_schema = rmcp::handler::server::tool::schema_for_type::<McpToolOutput<PushRecordListResult>>())]
    pub(super) async fn message_inbox(
        &self,
        Parameters(request): Parameters<PushRecordListParams>,
    ) -> CallToolResult {
        application_result(
            self.application.messages().message_inbox(request).await,
            OperationEffect::None,
        )
    }

    #[tool(name = "message_history", description = "Lists retained direct messages between the reported caller session and the explicit with session, including canonical sender status for outgoing remote mail. Caller identity is reported, not authenticated, and checked as a confusion guard. Read context is checked before SDK status; no sender push is invented.", output_schema = rmcp::handler::server::tool::schema_for_type::<McpToolOutput<collaboration_protocol::MessageHistoryResult>>())]
    pub(super) async fn message_history(
        &self,
        Parameters(request): Parameters<PushRecordHistoryParams>,
    ) -> CallToolResult {
        application_result(
            self.application.messages().message_history(request).await,
            OperationEffect::None,
        )
    }

    #[tool(name = "message_reply", description = "Replies to one direct message by its stored push id or router link, and records that reference on the reply. Caller identity is reported by the MCP request, not authenticated; the service uses it as a confusion guard. The result names the selected recipient.", output_schema = rmcp::handler::server::tool::schema_for_type::<McpToolOutput<MessageReplyResult>>())]
    pub(super) async fn message_reply(
        &self,
        Parameters(request): Parameters<MessageReplyRequest>,
    ) -> CallToolResult {
        if let Some(refusal) = self.sender_claim_refusal(&request.caller) {
            return refusal;
        }
        let caller = request.caller.clone();
        let params = SessionMessageReplyParams {
            caller: request.caller,
            reference: request.reference,
            text: request.text,
        };
        match self.application.messages().message_reply(params).await {
            Ok(reply) => message_reply_receipt_result(reply),
            Err(rejection) => message_failure_result(&rejection, "caller", caller),
        }
    }
}

/// A refused send or reply. Refused before anything was stored, it had no effect and names no
/// session; once stored, its delivery is unknown and it names the session it concerned, as
/// `field`.
pub(super) fn message_failure_result(
    rejection: &MessageFailure,
    field: &str,
    session: collaboration_protocol::SessionRef,
) -> CallToolResult {
    if rejected_before_submission(rejection) {
        failure_naming(
            &rejection_failure(rejection, OperationEffect::None),
            field,
            None,
        )
    } else {
        failure_naming(
            &rejection_failure(rejection, OperationEffect::Unknown),
            field,
            Some(session),
        )
    }
}

/// Whether the message was refused before anything was stored or sent.
fn rejected_before_submission(rejection: &MessageFailure) -> bool {
    match rejection.kind {
        MessageFailureKind::ItemConflict
        | MessageFailureKind::MailboxFull
        | MessageFailureKind::InvalidShape
        | MessageFailureKind::InvalidField
        | MessageFailureKind::WrongService
        | MessageFailureKind::Unavailable
        | MessageFailureKind::NotFound
        | MessageFailureKind::NotPermitted
        | MessageFailureKind::NotDirectMessage
        | MessageFailureKind::OwnerReplyUnsupported => true,
        MessageFailureKind::ForeignMachine | MessageFailureKind::OutcomeUnknown => false,
    }
}

// Match the existing undecoded-board boundary pattern without moving ID
// validation into a second wire type. The published schema stays the typed request.
#[derive(serde::Deserialize)]
#[serde(transparent)]
pub(super) struct UndecodedMessageSend(serde_json::Map<String, serde_json::Value>);
impl schemars::JsonSchema for UndecodedMessageSend {
    fn inline_schema() -> bool {
        <MessageSendRequest as schemars::JsonSchema>::inline_schema()
    }
    fn schema_name() -> std::borrow::Cow<'static, str> {
        <MessageSendRequest as schemars::JsonSchema>::schema_name()
    }
    fn schema_id() -> std::borrow::Cow<'static, str> {
        <MessageSendRequest as schemars::JsonSchema>::schema_id()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        <MessageSendRequest as schemars::JsonSchema>::json_schema(generator)
    }
}
