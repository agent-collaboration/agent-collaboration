//! A fresh issuer token renews the existing machine connection.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenewMachineToken {
    pub token: String,
}

/// Central's answer to `RenewMachineToken` on an open connection (PD:980).
/// After a refusal the Router refreshes once more; after a second refusal, or
/// on `enrollmentNotActive`, it asks for a new device code.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineTokenRenewal {
    Renewed {},
    Refused { reason: MachineTokenRenewalRefusal },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineTokenRenewalRefusal {
    TokenInvalid {},
    TokenExpired {},
    WrongClient {},
    WrongInstallation {},
    /// Revoked, expired or superseded.
    EnrollmentNotActive {},
    /// Central couldn't check the token right now: the issuer's keys were out of
    /// reach, or its own store was. Retrying is safe (owner decision N11).
    Unavailable {},
}
