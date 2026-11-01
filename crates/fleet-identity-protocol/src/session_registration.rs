//! Registration records launcher assertions without enforcing isolation.
use crate::{
    AbsolutePath, CommitId, IsolationInstanceId, ModelName, OrganizationId, RemoteStartId,
    RepositoryLocator,
};
use message_board::{ProjectId, SessionRef, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterFleetSession {
    pub session: SessionRef,
    pub project_id: ProjectId,
    pub started_by: StartedBy,
    pub isolation: AgentIsolation,
    pub launch_context: LaunchContext,
    pub machine_environment: MachineEnvironment,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum StartedBy {
    RouterOperator {},
    Session {
        session: SessionRef,
    },
    RemoteSession {
        session: SessionRef,
        request_id: RemoteStartId,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RegisterFleetSessionResult {
    Registered {
        organization_id: OrganizationId,
        workspace_id: WorkspaceId,
    },
    Refused {
        reason: SessionRefusal,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SessionRefusal {
    ProjectUnknown {},
    MachineNotAllowed {},
    AlreadyEnded {},
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentIsolation {
    pub kind: IsolationKind,
    pub instance_id: IsolationInstanceId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum IsolationKind {
    Host,
    ProcessSandbox,
    Container,
    Vm,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MachineEnvironment {
    Physical,
    CloudVm,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchContext {
    pub working_directory: AbsolutePath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<RepositoryLocator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<CommitId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelName>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EndFleetSession {
    pub organization_id: OrganizationId,
    pub session: SessionRef,
    pub cause: SessionEndCause,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SessionEndCause {
    Archived,
    Deleted,
}
