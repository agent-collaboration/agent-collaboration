//! Permissions central accepts from the sign-in service's live route check.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum FleetPermission {
    #[serde(rename = "workspace:create")]
    CreateWorkspace,
    #[serde(rename = "workspaceAdmin:allowMachine")]
    AllowWorkspaceMachine,
    #[serde(rename = "workspaceAdmin:withdrawMachine")]
    WithdrawWorkspaceMachine,
    #[serde(rename = "workspaceAdmin:removeSession")]
    RemoveWorkspaceSession,
    #[serde(rename = "machine:enroll")]
    EnrollMachine,
    #[serde(rename = "machine:revoke")]
    RevokeMachine,
    #[serde(rename = "machine:setTokenLifetime")]
    SetMachineTokenLifetime,
    #[serde(rename = "organization:view")]
    ViewOrganization,
    #[serde(rename = "organization:readAudit")]
    ReadOrganizationAudit,
}
