//! Live remote starts preserve correlation and ambiguous outcomes.
use crate::{AbsolutePath, InstallationId, OrganizationId, RemoteStartId, UnixSeconds};
use message_board::{MachineLabel, ProjectId, SessionRef, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestRemoteStart {
    pub request_id: RemoteStartId,
    pub requester: SessionRef,
    pub organization_id: OrganizationId,
    pub workspace_id: WorkspaceId,
    pub target: InstallationId,
    pub project_id: ProjectId,
    pub launch: RemoteLaunch,
    pub timeout_seconds: u32,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteLaunch {
    pub harness: HarnessKind,
    pub working_directory: AbsolutePath,
    pub requested_policy: RemoteLaunchPolicy,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum HarnessKind {
    Codex,
    Claude,
    Cursor,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteLaunchPolicy {
    pub access: RemoteLaunchAccess,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteLaunchAccess {
    WriteRestricted,
    WorkspaceWrite,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteStartAdmission {
    Forwarded {
        request_id: RemoteStartId,
        deadline: UnixSeconds,
    },
    Refused {
        reason: RemoteStartRefusal,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteStartRefusal {
    NotMember {},
    ProjectNotInWorkspace {},
    MachineNotAllowed {},
    MachineNotConnected {},
    RequestIdReused {},
    Unavailable {},
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartRequestedSession {
    pub request_id: RemoteStartId,
    pub requester: SessionRef,
    pub workspace_id: WorkspaceId,
    pub project_id: ProjectId,
    pub launch: RemoteLaunch,
    pub deadline: UnixSeconds,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReportRemoteStart {
    pub request_id: RemoteStartId,
    pub outcome: RemoteStartOutcome,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteStartOutcome {
    Started { session: SessionRef },
    RefusedByTarget { reason: TargetStartRefusal },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TargetStartRefusal {
    WorkingDirectoryMissing {},
    StartCheckRefused {},
    DeadlinePassed {},
    ProviderCreateFailed {},
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteStartAnswer {
    pub request_id: RemoteStartId,
    pub result: RemoteStartResult,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteStartResult {
    Started { session: SessionRef, late: bool },
    Refused { reason: RemoteStartRefusal },
    RefusedByTarget { reason: TargetStartRefusal },
    Unknown {},
    Pending {},
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteStartStatus {
    pub request_id: RemoteStartId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListWorkspaceMachines {
    pub organization_id: OrganizationId,
    pub workspace_id: WorkspaceId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceMachine {
    pub installation: InstallationId,
    pub label: MachineLabel,
}
