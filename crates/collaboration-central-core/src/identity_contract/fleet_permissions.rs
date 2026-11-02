//! Permissions central accepts from the sign-in service's live route check.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
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

// Serde's derived unit enum also accepts maps; the wire contract is a string.
impl<'de> Deserialize<'de> for FleetPermission {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let permission = String::deserialize(deserializer)?;
        match permission.as_str() {
            "workspace:create" => Ok(Self::CreateWorkspace),
            "workspaceAdmin:allowMachine" => Ok(Self::AllowWorkspaceMachine),
            "workspaceAdmin:withdrawMachine" => Ok(Self::WithdrawWorkspaceMachine),
            "workspaceAdmin:removeSession" => Ok(Self::RemoveWorkspaceSession),
            "machine:enroll" => Ok(Self::EnrollMachine),
            "machine:revoke" => Ok(Self::RevokeMachine),
            "machine:setTokenLifetime" => Ok(Self::SetMachineTokenLifetime),
            "organization:view" => Ok(Self::ViewOrganization),
            "organization:readAudit" => Ok(Self::ReadOrganizationAudit),
            _ => Err(serde::de::Error::custom("unknown Fleet permission")),
        }
    }
}
