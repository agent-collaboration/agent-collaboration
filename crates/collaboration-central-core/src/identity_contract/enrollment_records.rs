//! Observable enrollment states always retain the machine client they authorized.
use super::{AdmissionId, ClientId, DeviceLoginRequestId, PersonId};
use fleet_identity_protocol::{EnrollmentId, InstallationId, UnixSeconds};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AdmissionRef {
    Admission { admission_id: AdmissionId },
    Machine { installation: InstallationId },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EnrollmentAuthorization {
    DeviceCodeApproval {
        person_id: PersonId,
        request_id: DeviceLoginRequestId,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineEnrollment {
    Enrolled {
        enrollment_id: EnrollmentId,
        installation: InstallationId,
        authorized_by: EnrollmentAuthorization,
        machine_client_id: ClientId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ends_at: Option<UnixSeconds>,
    },
    Revoked {
        enrollment_id: EnrollmentId,
        installation: InstallationId,
        authorized_by: EnrollmentAuthorization,
        machine_client_id: ClientId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ends_at: Option<UnixSeconds>,
        reason: EnrollmentRevocationReason,
    },
    Expired {
        enrollment_id: EnrollmentId,
        installation: InstallationId,
        authorized_by: EnrollmentAuthorization,
        machine_client_id: ClientId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ends_at: Option<UnixSeconds>,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum EnrollmentRevocationReason {
    Owner {},
    Superseded {},
}
