//! The write as accepted by the Router. Only the outbox claim adds send time.
use crate::{OutboxWriteContent, OutboxWriteTarget};
use central_link_protocol::{RequestPayload, RequestScope};
use serde::{Deserialize, Serialize};
use workspace_model::{
    RequestId, Timestamp, WorkspaceRequest,
    central_mail::{CentralMailOperation, CentralMailRequest, SendMessageRequest},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "request",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum QueuedWrite {
    Workspace(WorkspaceRequest),
    SendMessage {
        request_id: RequestId,
        message: SendMessageRequest,
    },
}

impl QueuedWrite {
    pub fn request_id(&self) -> Option<&RequestId> {
        match self {
            Self::Workspace(request) => request.request_id.as_ref(),
            Self::SendMessage { request_id, .. } => Some(request_id),
        }
    }

    pub fn request_scope(&self) -> RequestScope {
        match self {
            Self::Workspace(request) => request
                .workspace_id
                .clone()
                .map_or(RequestScope::Organization, RequestScope::Workspace),
            Self::SendMessage { .. } => RequestScope::Organization,
        }
    }

    pub fn content(&self) -> Option<OutboxWriteContent> {
        match self {
            Self::Workspace(request) => OutboxWriteContent::workspace(request),
            Self::SendMessage { message, .. } => Some(OutboxWriteContent {
                target: OutboxWriteTarget::Session {
                    to: message.to.clone(),
                },
                body: message.body.clone(),
            }),
        }
    }

    pub fn into_wire(self, sent_at: Timestamp) -> RequestPayload {
        match self {
            Self::Workspace(request) => RequestPayload::Workspace(request),
            Self::SendMessage {
                request_id,
                message,
            } => RequestPayload::Mail(CentralMailRequest {
                request_id,
                operation: CentralMailOperation::SendMessage {
                    to: message.to,
                    body: message.body,
                    delivery: message.delivery,
                    reply_to: message.reply_to,
                    sent_at,
                },
            }),
        }
    }
}

#[cfg(test)]
#[path = "queued_write_test.rs"]
mod tests;
