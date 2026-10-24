//! Edge DTOs for the live enrollment and Fleet import owners.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RegistryCall {
    EnrollMachine(crate::EnrollMachine),
    RevokeEnrollment {
        installation: message_board::ServiceId,
    },
    AuthenticateEnrollment {
        credential_hash: String,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RegistryResult {
    Enrollment(crate::EnrollmentDecision),
    EnrollmentRevoked(crate::RevocationDecision),
    EnrollmentAuthenticated(Result<crate::AdmittedMachine, fleet_sync_protocol::AuthRefusal>),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum FleetCall {
    Import {
        bundle: Box<fleet_admission::FleetImportBundle>,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum FleetResult {
    Imported(fleet_admission::FleetImportResult),
}
