//! Sign-in service decisions and machine-token extension claims.
use super::{ClientId, DeviceLoginRequestId, PersonId};
use fleet_identity_protocol::{EnrollmentId, InstallationId, OrganizationId, UnixSeconds};
use fleet_sync_protocol::PeerId;
use message_board::{MachineLabel, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecideDeviceLogin {
    pub organization_id: OrganizationId,
    pub approver: PersonId,
    pub request_id: DeviceLoginRequestId,
    pub installation: InstallationId,
    #[schemars(with = "message_board::RecordId")]
    pub peer: PeerId,
    pub label: MachineLabel,
    pub allowed_workspaces: Vec<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver_second_factor_at: Option<UnixSeconds>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DeviceLoginDecision {
    Approved { enrollment_id: EnrollmentId },
    Refused { reason: DeviceLoginRefusal },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DeviceLoginRefusal {
    NotOwner {},
    MachineInAnotherOrganization {},
    UnknownWorkspace {},
    SecondFactorRequired { within_seconds: u32 },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecideMachineToken {
    pub organization_id: OrganizationId,
    pub enrollment_id: EnrollmentId,
    pub client_id: ClientId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineTokenDecision {
    Grant {
        installation: InstallationId,
        enrollment_id: EnrollmentId,
        decided_at: UnixSeconds,
        grant_expires_at: UnixSeconds,
    },
    Refuse {
        reason: MachineTokenRefusal,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineTokenRefusal {
    EnrollmentNotActive {},
    Superseded {},
    WrongClient {},
    IdleExpired {},
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PresentedMachine {
    pub installation: InstallationId,
    #[schemars(with = "message_board::RecordId")]
    pub peer: PeerId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmitMachineConnection {
    pub organization_id: OrganizationId,
    pub enrollment_id: EnrollmentId,
    pub installation: InstallationId,
    pub presented: PresentedMachine,
    pub token_expires_at: UnixSeconds,
    pub client_id: ClientId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmittedMachine {
    pub installation: InstallationId,
    #[schemars(with = "message_board::RecordId")]
    pub peer: PeerId,
    pub organization_id: OrganizationId,
    pub enrollment_id: EnrollmentId,
    pub token_expires_at: UnixSeconds,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineConnectionAdmission {
    Admitted { machine: AdmittedMachine },
    Refused { reason: MachineConnectionRefusal },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineConnectionRefusal {
    UnknownMachine {},
    EnrollmentNotActive {},
    Superseded {},
    MachineInAnotherOrganization {},
    WrongClient {},
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "operation",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PersonAdmissionRequest {
    AdmitSignIn {
        organization_id: OrganizationId,
        person_id: PersonId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        verified_email: Option<String>,
    },
    Standing {
        organization_id: OrganizationId,
        person_id: PersonId,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OrganizationStanding {
    Admitted { role: StandingRole },
    AwaitingOwner {},
    NotAdmitted {},
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StandingRole {
    Member,
    Owner,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisableMachineClient {
    pub organization_id: OrganizationId,
    pub enrollment_id: EnrollmentId,
    pub client_id: ClientId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DisabledMachineClient {
    Disabled {},
    AlreadyDisabled {},
    NotThisEnrollment {},
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "snake_case",
    deny_unknown_fields
)]
pub enum MachinePrincipal {
    Machine {
        installation_id: InstallationId,
        enrollment_id: EnrollmentId,
    },
}
/// Only extension claims live here; the token checker validates the registered claims separately.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MachineTokenClaims {
    pub principal: MachinePrincipal,
    pub organization_id: OrganizationId,
    pub grant_expires_at: UnixSeconds,
}
