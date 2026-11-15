use crate::ProtocolError;
use serde::{Deserialize, Serialize};
use workspace_model::central_mail::{CentralMailRequest, CentralMailResult};
use workspace_model::{Caller, RequestId, WorkspaceReply, WorkspaceRequest};

/// Transport correlation has a separate type and lifetime from durable write idempotency.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CorrelationId(String);

impl CorrelationId {
    /// The client supplies time from its injected clock and its one UUIDv7 context.
    pub fn mint(timestamp: uuid::Timestamp) -> Self {
        Self(uuid::Uuid::new_v7(timestamp).to_string())
    }
}

impl TryFrom<String> for CorrelationId {
    type Error = workspace_ids::IdValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let id: workspace_ids::RequestId = value.parse()?;
        Ok(Self(id.into()))
    }
}
impl From<CorrelationId> for String {
    fn from(value: CorrelationId) -> Self {
        value.0
    }
}

impl std::str::FromStr for CorrelationId {
    type Err = workspace_ids::IdValidationError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value.to_owned())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "request",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RequestPayload {
    Workspace(WorkspaceRequest),
    Mail(CentralMailRequest),
}

impl RequestPayload {
    pub fn write_request_id(&self) -> Option<&RequestId> {
        match self {
            Self::Workspace(request) => request.request_id.as_ref(),
            Self::Mail(request) => Some(&request.request_id),
        }
    }

    fn validate(&self) -> Result<(), ProtocolError> {
        if let Self::Workspace(request) = self
            && request.operation.is_write() != request.request_id.is_some()
        {
            return Err(ProtocolError::RequestIdPolicy { field: "requestId" });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "reply",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ReplyPayload {
    Workspace(WorkspaceReply),
    Mail(CentralMailResult),
}

impl ReplyPayload {
    pub fn matches_request(&self, request: &RequestPayload) -> bool {
        match (self, request) {
            (Self::Workspace(_), RequestPayload::Workspace(_))
            | (Self::Mail(_), RequestPayload::Mail(_)) => true,
            (Self::Workspace(_), RequestPayload::Mail(_))
            | (Self::Mail(_), RequestPayload::Workspace(_)) => false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RequestWire", into = "RequestWire")]
pub struct LinkRequest {
    correlation_id: CorrelationId,
    caller: Caller,
    payload: RequestPayload,
}

impl LinkRequest {
    pub fn new(
        correlation_id: CorrelationId,
        caller: Caller,
        payload: RequestPayload,
    ) -> Result<Self, ProtocolError> {
        payload.validate()?;
        Ok(Self {
            correlation_id,
            caller,
            payload,
        })
    }
    pub fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }
    pub fn caller(&self) -> &Caller {
        &self.caller
    }
    pub fn payload(&self) -> &RequestPayload {
        &self.payload
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RequestWire {
    correlation_id: CorrelationId,
    caller: Caller,
    payload: RequestPayload,
}

impl TryFrom<RequestWire> for LinkRequest {
    type Error = ProtocolError;
    fn try_from(wire: RequestWire) -> Result<Self, Self::Error> {
        Self::new(wire.correlation_id, wire.caller, wire.payload)
    }
}
impl From<LinkRequest> for RequestWire {
    fn from(request: LinkRequest) -> Self {
        Self {
            correlation_id: request.correlation_id,
            caller: request.caller,
            payload: request.payload,
        }
    }
}

#[cfg(test)]
#[path = "link_request_test.rs"]
mod tests;
