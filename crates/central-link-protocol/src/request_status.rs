//! The caller's reference to a request in one owning store.
use crate::ReplyPayload;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use workspace_ids::{RequestId, WorkspaceId};
use workspace_model::{RefusalReason, central_mail::CentralMailRefusal};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "workspaceId",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RequestScope {
    Organization,
    Workspace(WorkspaceId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestStatusQuery {
    pub request_id: RequestId,
    pub scope: RequestScope,
}

/// The stored decision, scoped to the original caller and owning store.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RequestStatusAnswer {
    Answered(ReplyPayload),
    Refused(RequestRefusal),
    NotFound,
}

/// A domain refusal recorded in the request log.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "reason",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RequestRefusal {
    Workspace(RefusalReason),
    Mail(CentralMailRefusal),
}

#[cfg(test)]
#[path = "request_status_test.rs"]
mod tests;
