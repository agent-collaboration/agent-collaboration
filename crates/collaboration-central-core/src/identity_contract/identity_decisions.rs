//! Sign-in service decisions and machine-token extension claims.
use super::{
    AllowedWorkspaces, ClientId, DeviceLoginLabel, DeviceLoginRequestId, EnrollPermission,
    FormerServiceId, PersonId,
};
use fleet_identity_protocol::{EnrollmentId, InstallationId, OrganizationId, UnixSeconds};
use fleet_sync_protocol::PeerId;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::num::NonZeroU32;

/// The issuer asks central to approve a device login. `approverSecondFactorAt`
/// is always present and may be null; `formerServiceId` is present only for a
/// machine re-keying from an older service identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecideDeviceLogin {
    pub organization_id: OrganizationId,
    pub approver: PersonId,
    pub permission_checked: EnrollPermission,
    pub request_id: DeviceLoginRequestId,
    pub installation: InstallationId,
    #[schemars(with = "message_board::RecordId")]
    pub peer: PeerId,
    pub label: DeviceLoginLabel,
    pub allowed_workspaces: AllowedWorkspaces,
    #[serde(deserialize_with = "present_nullable")]
    #[schemars(required, schema_with = "nullable_unix_seconds")]
    pub approver_second_factor_at: Option<UnixSeconds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub former_service_id: Option<FormerServiceId>,
}
/// Makes an `Option` field's key required while its value may be null.
fn present_nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(deserializer)
}
/// `approverSecondFactorAt`'s schema: a required key whose value may be null.
fn nullable_unix_seconds(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let seconds = generator.subschema_for::<UnixSeconds>();
    schemars::json_schema!({"anyOf":[seconds,{"type":"null"}]})
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
/// `formerServiceLinkedElsewhere` says only that the former service id cannot
/// be linked here; it names no organization, enrollment or installation.
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
    FormerServiceLinkedElsewhere {},
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecideMachineToken {
    pub organization_id: OrganizationId,
    pub enrollment_id: EnrollmentId,
    pub client_id: ClientId,
}
/// The enrollment and OAuth client a first grant ended by superseding them.
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
        decided_at: UnixSeconds,
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
