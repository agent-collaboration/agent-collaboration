//! Transient conversation calls carried by the existing opaque Fleet envelope.
use collaboration_mail_protocol::RemoteCreateRequest;
use message_board::RequestId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteCreateCall {
    #[serde(rename = "remoteCreate")]
    Create(Box<RemoteCreateRequest>),
    #[serde(rename = "remoteCreateStatus")]
    Status { request_id: RequestId },
}

impl RemoteCreateCall {
    #[must_use]
    pub fn request_id(&self) -> &RequestId {
        match self {
            Self::Create(request) => &request.request_id,
            Self::Status { request_id } => request_id,
        }
    }
}

#[cfg(test)]
#[path = "remote_create_calls_test.rs"]
mod tests;
