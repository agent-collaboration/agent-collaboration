//! Shared message request preparation for CLI and MCP callers.
use crate::{
    ClientError, CollaborationClient, OperationEffect, OperationFailure,
    operation_failure_from_client_error,
};
use collaboration_protocol::{
    CodexGeneration, MessageContent, MessageDelivery, MessageReplyResult, MessageSendResult,
    MessageText, SessionMessageReplyParams, SessionMessageSendParams, SessionRef,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageSendRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<collaboration_protocol::MailItemId>,
    pub target: SessionRef,
    pub message: PublicMessageContent,
    #[serde(default)]
    pub delivery: MessageDelivery,
    pub generation_guard: Option<CodexGeneration>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageReplyRequest {
    pub caller: SessionRef,
    pub reference: String,
    pub text: MessageText,
}

#[derive(Debug, thiserror::Error)]
pub enum MessageSendError {
    #[error(transparent)]
    Preparation(Box<ClientError>),
    #[error("{source}")]
    Submission {
        target: SessionRef,
        #[source]
        source: Box<ClientError>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum MessageReplyError {
    #[error(transparent)]
    Preparation(Box<ClientError>),
    #[error("{source}")]
    Submission {
        caller: SessionRef,
        #[source]
        source: Box<ClientError>,
    },
}

impl MessageReplyError {
    #[must_use]
    pub fn into_operation_failure_and_caller(self) -> (OperationFailure, Option<SessionRef>) {
        match self {
            Self::Preparation(error) => (
                operation_failure_from_client_error(*error, OperationEffect::None),
                None,
            ),
            Self::Submission { caller, source } => {
                let failure =
                    operation_failure_from_client_error(*source, OperationEffect::Unknown);
                let affected = (failure.effect != OperationEffect::None).then_some(caller);
                (failure, affected)
            }
        }
    }
}

impl MessageSendError {
    #[must_use]
    pub fn into_operation_failure_and_target(self) -> (OperationFailure, Option<SessionRef>) {
        match self {
            Self::Preparation(error) => (
                operation_failure_from_client_error(*error, OperationEffect::None),
                None,
            ),
            Self::Submission { target, source } => {
                let failure =
                    operation_failure_from_client_error(*source, OperationEffect::Unknown);
                let affected = (failure.effect != OperationEffect::None).then_some(target);
                (failure, affected)
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum PublicMessageContent {
    Agent {
        sender: SessionRef,
        text: collaboration_protocol::MessageText,
    },
    HumanUser {
        text: collaboration_protocol::MessageText,
    },
}

impl TryFrom<MessageContent> for PublicMessageContent {
    type Error = ClientError;
    fn try_from(value: MessageContent) -> Result<Self, Self::Error> {
        match value {
            MessageContent::Agent { sender, text } => Ok(Self::Agent { sender, text }),
            MessageContent::HumanUser { text } => Ok(Self::HumanUser { text }),
            MessageContent::Router { .. } => Err(ClientError::InvalidRequest(
                "Router message content is internal-only",
            )),
        }
    }
}

impl From<PublicMessageContent> for MessageContent {
    fn from(value: PublicMessageContent) -> Self {
        match value {
            PublicMessageContent::Agent { sender, text } => Self::Agent { sender, text },
            PublicMessageContent::HumanUser { text } => Self::HumanUser { text },
        }
    }
}

impl CollaborationClient {
    pub async fn send_message(
        &self,
        request: MessageSendRequest,
    ) -> Result<MessageSendResult, MessageSendError> {
        if let PublicMessageContent::Agent { sender, .. } = &request.message
            && sender.endpoint.service_id != self.identity().service_id
        {
            return Err(MessageSendError::Preparation(Box::new(
                ClientError::InvalidRequest("message sender belongs to another service"),
            )));
        }
        let target = request.target.clone();
        let params = SessionMessageSendParams {
            item_id: request.item_id,
            target: request.target,
            message: request.message.into(),
            mode: request.delivery,
            generation_guard: request.generation_guard,
        };
        self.submit_message(params)
            .await
            .map_err(|source| MessageSendError::Submission {
                target,
                source: Box::new(source),
            })
    }

    pub async fn reply_to_push(
        &self,
        request: MessageReplyRequest,
    ) -> Result<MessageReplyResult, MessageReplyError> {
        let caller = request.caller.clone();
        let params = SessionMessageReplyParams {
            caller: request.caller,
            reference: request.reference,
            text: request.text,
        };
        match self.message_reply(params).await {
            Ok(receipt) => Ok(receipt),
            Err(error) if is_reply_pre_dispatch_rejection(&error) => {
                Err(MessageReplyError::Preparation(Box::new(error)))
            }
            Err(error) => Err(MessageReplyError::Submission {
                caller,
                source: Box::new(error),
            }),
        }
    }

    pub async fn router_show(
        &self,
        request: impl Into<collaboration_protocol::MessageShowRequest>,
    ) -> Result<collaboration_protocol::MessageShowResult, ClientError> {
        let request = request.into();
        if let collaboration_protocol::MessageReadCaller::Session(caller) = &request.caller
            && caller.endpoint.service_id != self.identity().service_id
        {
            return Err(ClientError::InvalidRequest(
                "push show caller belongs to another service",
            ));
        }
        let value = self
            .connection
            .call("router_show", serde_json::json!(request))
            .await?;
        serde_json::from_value(value)
            .map_err(|_| ClientError::Protocol("invalid Router push show result"))
    }

    pub async fn message_inbox(
        &self,
        request: collaboration_protocol::PushRecordListParams,
    ) -> Result<collaboration_protocol::PushRecordListResult, ClientError> {
        if request.caller.endpoint.service_id != self.identity().service_id {
            return Err(ClientError::InvalidRequest(
                "message inbox caller belongs to another service",
            ));
        }
        let value = self
            .connection
            .call("message_inbox", serde_json::json!(request))
            .await?;
        serde_json::from_value(value)
            .map_err(|_| ClientError::Protocol("invalid message inbox result"))
    }

    pub async fn message_history(
        &self,
        request: collaboration_protocol::PushRecordHistoryParams,
    ) -> Result<collaboration_protocol::MessageHistoryResult, ClientError> {
        if request.caller.endpoint.service_id != self.identity().service_id {
            return Err(ClientError::InvalidRequest(
                "message history caller must belong to this service",
            ));
        }
        let value = self
            .connection
            .call("message_history", serde_json::json!(request))
            .await?;
        serde_json::from_value(value)
            .map_err(|_| ClientError::Protocol("invalid message history result"))
    }
}

fn is_reply_pre_dispatch_rejection(error: &ClientError) -> bool {
    match error {
        ClientError::InvalidRequest(_) | ClientError::Overloaded { .. } => true,
        ClientError::Rejected {
            data: Some(data), ..
        } => matches!(
            data.get("kind").and_then(serde_json::Value::as_str),
            Some(
                "wrongService"
                    | "unavailable"
                    | "notFound"
                    | "notPermitted"
                    | "notDirectMessage"
                    | "ownerReplyUnsupported"
                    | "invalidField"
                    | "mailboxFull"
            )
        ),
        ClientError::Rejected { data: None, .. } => false,
        ClientError::Discovery { .. }
        | ClientError::Transport(_)
        | ClientError::Protocol(_)
        | ClientError::Timeout
        | ClientError::UnsupportedCapability(_) => false,
    }
}

#[cfg(test)]
#[path = "message_operation_tests.rs"]
mod tests;
