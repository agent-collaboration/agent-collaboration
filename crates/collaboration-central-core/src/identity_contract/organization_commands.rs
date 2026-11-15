//! Organization commands share one refusal vocabulary and keep reads separate.
use super::CommandId;
use fleet_identity_protocol::{InstallationId, OrganizationId};
use message_board::{ResourceName, SessionRef, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateOrganization {
    pub organization_id: OrganizationId,
    pub owner_verified_email: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetOrganizationOwner {
    pub organization_id: OrganizationId,
    pub owner_verified_email: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWorkspace {
    pub organization_id: OrganizationId,
    pub name: ResourceName,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AllowSignIn {
    pub organization_id: OrganizationId,
    pub verified_email: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveSignIn {
    pub organization_id: OrganizationId,
    pub verified_email: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AllowMachine {
    pub organization_id: OrganizationId,
    pub installation: InstallationId,
    pub workspace_id: WorkspaceId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WithdrawMachineAllowance {
    pub organization_id: OrganizationId,
    pub installation: InstallationId,
    pub workspace_id: WorkspaceId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeMachineEnrollment {
    pub organization_id: OrganizationId,
    pub installation: InstallationId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveSession {
    pub organization_id: OrganizationId,
    pub session: SessionRef,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OrganizationCommandResult {
    Applied {},
    Admitted { command_id: CommandId },
    Refused { reason: CommandRefusal },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CommandRefusal {
    NotOwner {},
    NotFound {},
    Conflict {},
    RecordUnwritable {},
    SecondFactorRequired { within_seconds: u32 },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MachineAllowance {
    pub installation: InstallationId,
    pub workspace_id: WorkspaceId,
    pub state: AllowanceState,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AllowanceState {
    Active,
    Withdrawn,
}
