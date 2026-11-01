//! Live start admission precedes provider creation and records no session.
use crate::StartedBy;
use message_board::{ProjectId, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckSessionStart {
    pub project_id: ProjectId,
    pub started_by: StartedBy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SessionStartCheck {
    Allowed { workspace_id: WorkspaceId },
    Refused { reason: SessionStartRefusal },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SessionStartRefusal {
    ProjectUnknown {},
    MachineNotAllowed {},
    DifferentWorkspace {},
    StarterNotActive {},
}
