//! Sign-in service decisions and machine-token extension claims.
use super::{AllowedWorkspaces, ClientId, DeviceLoginRequestId, EnrollPermission, PersonId};
use fleet_identity_protocol::{EnrollmentId, InstallationId, OrganizationId, UnixSeconds};
use super::PeerId;
use message_board::{MachineLabel, RecordId};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::num::NonZeroU32;
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecideDeviceLogin {
    pub organization_id: OrganizationId,
    pub approver: PersonId,
    pub permission_checked: EnrollPermission,
    pub request_id: DeviceLoginRequestId,
    #[serde(deserialize_with = "uuid_v7_installation")]
    #[schemars(with = "RecordId")]
    pub installation: InstallationId,
    #[schemars(with = "message_board::RecordId")]
    pub peer: PeerId,
    pub label: MachineLabel,
    pub allowed_workspaces: AllowedWorkspaces,
    #[serde(deserialize_with = "nullable_nonnegative_unix_seconds")]
    #[schemars(required, schema_with = "Option::<UnixSeconds>::json_schema", extend("minimum" = 0))]
    pub approver_second_factor_at: Option<UnixSeconds>,
    #[serde(deserialize_with = "nonnegative_unix_seconds")]
    #[schemars(with = "UnixSeconds", extend("minimum" = 0))]
    pub code_expires_at: UnixSeconds,
}
fn nonnegative_unix_seconds<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<UnixSeconds, D::Error> {
    let seconds = UnixSeconds::deserialize(deserializer)?;
    if seconds.0 < 0 {
        return Err(serde::de::Error::custom("Unix seconds must be nonnegative"));
    }
    Ok(seconds)
}
fn nullable_nonnegative_unix_seconds<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<UnixSeconds>, D::Error> {
    let seconds = Option::<UnixSeconds>::deserialize(deserializer)?;
    if seconds.is_some_and(|seconds| seconds.0 < 0) {
        return Err(serde::de::Error::custom("Unix seconds must be nonnegative"));
    }
    Ok(seconds)
}
fn uuid_v7_installation<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<InstallationId, D::Error> {
    let installation = RecordId::deserialize(deserializer)?;
    InstallationId::try_from(String::from(installation)).map_err(serde::de::Error::custom)
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DeviceLoginDecision {
    Approved {
        enrollment_id: EnrollmentId,
        label: MachineLabel,
        allowed_workspaces: AllowedWorkspaces,
    },
    Refused {
        reason: DeviceLoginRefusal,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DeviceLoginRefusal {
    NotPermitted {},
    MachineInAnotherOrganization {},
    UnknownWorkspace {},
    SecondFactorRequired { within_seconds: NonZeroU32 },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecideMachineToken {
    pub organization_id: OrganizationId,
    pub enrollment_id: EnrollmentId,
    pub client_id: ClientId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EndedMachineClient {
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
        #[serde(deserialize_with = "nonnegative_unix_seconds")]
        #[schemars(with = "UnixSeconds", extend("minimum" = 0))]
        decided_at: UnixSeconds,
        #[serde(deserialize_with = "nonnegative_unix_seconds")]
        #[schemars(with = "UnixSeconds", extend("minimum" = 0))]
        grant_expires_at: UnixSeconds,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        superseded: Option<EndedMachineClient>,
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

#[cfg(test)]
#[path = "identity_decisions_test.rs"]
mod tests;
