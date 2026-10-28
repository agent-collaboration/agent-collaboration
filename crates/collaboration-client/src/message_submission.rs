//! Message submission and validation of local carrier evidence or remote outbox custody.
use crate::api_connection::ToolAnswer;
use crate::{ClientError, CollaborationClient};
use serde_json::{Value, json};

impl CollaborationClient {
    /// Agent-originated communication; delivery defaults are carried by the typed request.
    pub async fn send_agent_message(
        &self,
        params: collaboration_protocol::SessionMessageSendParams,
    ) -> Result<collaboration_protocol::MessageSendResult, ClientError> {
        if !matches!(
            params.message,
            collaboration_protocol::MessageContent::Agent { .. }
        ) {
            return Err(ClientError::InvalidRequest("agent message required"));
        }
        self.submit_message(params).await
    }

    /// Explicit human input; does not manufacture an authenticated human identity.
    pub async fn send_human_input(
        &self,
        params: collaboration_protocol::SessionMessageSendParams,
    ) -> Result<collaboration_protocol::MessageSendResult, ClientError> {
        if !matches!(
            params.message,
            collaboration_protocol::MessageContent::HumanUser { .. }
        ) {
            return Err(ClientError::InvalidRequest("human input required"));
        }
        self.submit_message(params).await
    }

    /// Replies to one stored direct message using its push id or Router link.
    pub async fn message_reply(
        &self,
        params: collaboration_protocol::SessionMessageReplyParams,
    ) -> Result<collaboration_protocol::MessageReplyResult, ClientError> {
        if params.caller.endpoint.service_id != self.identity().service_id {
            return Err(ClientError::InvalidRequest(
                "reply caller belongs to another service",
            ));
        }
        let value = self
            .delivery_receipt("message_reply", json!(params))
            .await?;
        let result: collaboration_protocol::MessageReplyResult = serde_json::from_value(value)
            .map_err(|_| {
                ClientError::Protocol("invalid message reply result; acceptance unknown")
            })?;
        let consistent = match &result {
            collaboration_protocol::MessageSendResult::Local(reply) => {
                reply.target.endpoint.service_id == self.identity().service_id
                    && !reply.target_identity.trim().is_empty()
            }
            collaboration_protocol::MessageSendResult::Queued { queued } => {
                queued.target.endpoint.service_id != self.identity().service_id
                    && !queued.target_identity.trim().is_empty()
            }
        };
        if !consistent {
            return Err(ClientError::Protocol(
                "inconsistent message reply result; acceptance unknown",
            ));
        }
        Ok(result)
    }

    pub(crate) async fn submit_message(
        &self,
        params: collaboration_protocol::SessionMessageSendParams,
    ) -> Result<collaboration_protocol::MessageSendResult, ClientError> {
        use collaboration_protocol::{
            DeliveryClientReceipt, DeliveryOutcome, MessageDelivery, MessageInputKind,
            MessageRepresentation, NativeInputOperation, NativeSendAcceptance, SessionReachability,
        };
        if let collaboration_protocol::MessageContent::Agent { sender, .. } = &params.message
            && sender.endpoint.service_id != self.identity().service_id
        {
            return Err(ClientError::InvalidRequest(
                "message sender belongs to another service",
            ));
        }
        let message = crate::PublicMessageContent::try_from(params.message.clone())?;
        let value = self
            .delivery_receipt(
                "message_send",
                serde_json::to_value(crate::MessageSendRequest {
                    item_id: params.item_id.clone(),
                    target: params.target.clone(),
                    message,
                    delivery: params.mode,
                    generation_guard: params.generation_guard.clone(),
                })
                .map_err(|_| ClientError::InvalidRequest("message request"))?,
            )
            .await?;
        let decoded = serde_json::from_value::<collaboration_protocol::MessageSendResult>(value);
        let Ok(receipt) = decoded else {
            return Err(ClientError::Protocol(
                "invalid message receipt; acceptance unknown",
            ));
        };
        let receipt = match receipt {
            collaboration_protocol::MessageSendResult::Queued { queued } => {
                let consistent = params.target.endpoint.service_id != self.identity().service_id
                    && queued.target == params.target
                    && !queued.target_identity.trim().is_empty()
                    && params
                        .item_id
                        .as_ref()
                        .is_none_or(|item| item == &queued.item);
                if !consistent {
                    return Err(ClientError::Protocol(
                        "inconsistent queued message result; acceptance unknown",
                    ));
                }
                return Ok(collaboration_protocol::MessageSendResult::Queued { queued });
            }
            collaboration_protocol::MessageSendResult::Local(receipt) => receipt,
        };
        // The service turns either caller input kind into a Router-authored
        // push line. Native delivery renders that line as declared agent text.
        let kind = MessageInputKind::Agent;
        let representation = MessageRepresentation::DeclaredAgentText;
        let delivery_receipt = &receipt.receipt;
        let expected_link = collaboration_protocol::RouterLink::new(
            collaboration_protocol::MachineId::from(self.identity().service_id.clone()),
            receipt.push_id.clone(),
        )
        .to_string();
        let consistent = params.target.endpoint.service_id == self.identity().service_id
            && !receipt.target_identity.trim().is_empty()
            && params
                .item_id
                .as_ref()
                .is_none_or(|id| id.as_str() == receipt.push_id.as_str())
            && receipt.target == params.target
            && receipt.link == expected_link
            && match (&delivery_receipt.reachability, &delivery_receipt.client) {
                (None, None) => true,
                (
                    Some(SessionReachability::CodexAppServer),
                    Some(DeliveryClientReceipt::CodexAppServer(native)),
                ) => {
                    let acceptance_matches = matches!(
                        (&params.mode, &delivery_receipt.outcome, &native.acceptance),
                        (
                            MessageDelivery::Auto,
                            DeliveryOutcome::Started,
                            NativeSendAcceptance::NativeInputAccepted {
                                operation: NativeInputOperation::TurnStart,
                                ..
                            }
                        ) | (
                            MessageDelivery::Auto,
                            DeliveryOutcome::StartedOrSteered,
                            NativeSendAcceptance::NativeInputAccepted { .. }
                        ) | (
                            MessageDelivery::Auto,
                            DeliveryOutcome::Steered,
                            NativeSendAcceptance::SteerAccepted { .. }
                        ) | (
                            MessageDelivery::Queue,
                            DeliveryOutcome::Queued,
                            NativeSendAcceptance::QueueAccepted { .. }
                        ) | (
                            MessageDelivery::Steer,
                            DeliveryOutcome::Steered,
                            NativeSendAcceptance::SteerAccepted { .. }
                        )
                    );
                    native.target == params.target
                        && params
                            .generation_guard
                            .as_ref()
                            .is_none_or(|guard| guard == &native.generation)
                        && native.input_kind == kind
                        && native.representation == representation
                        && acceptance_matches
                        && String::from(native.client_user_message_id.clone())
                            == receipt.push_id.as_str()
                }
                (
                    Some(SessionReachability::ProviderAcp),
                    Some(DeliveryClientReceipt::ProviderAcp { .. }),
                ) => {
                    matches!(
                        delivery_receipt.outcome,
                        DeliveryOutcome::Started
                            | DeliveryOutcome::Steered
                            | DeliveryOutcome::Queued
                    )
                }
                (
                    Some(SessionReachability::ClaudeCodePeer),
                    Some(DeliveryClientReceipt::ClaudeCodePeer),
                ) => {
                    matches!(
                        delivery_receipt.outcome,
                        DeliveryOutcome::PeerMessageWritten
                    )
                }
                (Some(_), None) => matches!(
                    delivery_receipt.outcome,
                    DeliveryOutcome::NotSubmitted { .. }
                        | DeliveryOutcome::Rejected(_)
                        | DeliveryOutcome::Unknown
                ),
                _ => false,
            };
        if !consistent {
            return Err(ClientError::Protocol(
                "inconsistent message receipt; acceptance unknown",
            ));
        }
        Ok(collaboration_protocol::MessageSendResult::Local(receipt))
    }

    /// A stored push and its delivery receipt. A delivery the tool reports as an error (not
    /// submitted, refused or unknown) still carries the stored push, which the caller judges.
    async fn delivery_receipt(&self, tool: &str, arguments: Value) -> Result<Value, ClientError> {
        match self.connection.answer(tool, arguments).await? {
            ToolAnswer::Success(value) => Ok(value),
            ToolAnswer::Failure(mut failure)
                if failure.get("pushId").is_some() && failure.get("receipt").is_some() =>
            {
                if let Some(fields) = failure.as_object_mut() {
                    for presentation in ["mcpResult", "kind", "message", "effect"] {
                        fields.remove(presentation);
                    }
                }
                Ok(failure)
            }
            ToolAnswer::Failure(failure) => {
                Err(crate::api_connection::rejection_from_tool_failure(failure))
            }
        }
    }
}
