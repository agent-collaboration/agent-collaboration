//! The write as accepted by the Router. Only the outbox claim adds send time.
use crate::{OutboxWriteContent, OutboxWriteTarget};
use central_link_protocol::RequestPayload;
use serde::{Deserialize, Serialize};
use workspace_ids::SessionRef;
use workspace_model::{
    MessageId, PostText, RequestId, Timestamp, WorkspaceRequest,
    central_mail::{CentralMailOperation, CentralMailRequest, MessageDelivery},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolSendMessage {
    pub request_id: RequestId,
    pub to: SessionRef,
    pub body: PostText,
    pub delivery: MessageDelivery,
    pub reply_to: Option<MessageId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "request",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum QueuedWrite {
    Workspace(WorkspaceRequest),
    SendMessage(ToolSendMessage),
}

impl QueuedWrite {
    pub fn request_id(&self) -> Option<&RequestId> {
        match self {
            Self::Workspace(request) => request.request_id.as_ref(),
            Self::SendMessage(request) => Some(&request.request_id),
        }
    }

    pub fn content(&self) -> Option<OutboxWriteContent> {
        match self {
            Self::Workspace(request) => OutboxWriteContent::workspace(request),
            Self::SendMessage(request) => Some(OutboxWriteContent {
                target: OutboxWriteTarget::Session {
                    to: request.to.clone(),
                },
                body: request.body.clone(),
            }),
        }
    }

    pub fn into_wire(self, sent_at: Timestamp) -> RequestPayload {
        match self {
            Self::Workspace(request) => RequestPayload::Workspace(request),
            Self::SendMessage(request) => RequestPayload::Mail(CentralMailRequest {
                request_id: request.request_id,
                operation: CentralMailOperation::SendMessage {
                    to: request.to,
                    body: request.body,
                    delivery: request.delivery,
                    reply_to: request.reply_to,
                    sent_at,
                },
            }),
        }
    }
}

#[cfg(test)]
#[path = "queued_write_test.rs"]
mod tests;
