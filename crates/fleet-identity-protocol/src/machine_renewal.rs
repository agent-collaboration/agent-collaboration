//! A fresh issuer token renews the existing machine connection.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenewMachineToken {
    pub token: String,
}

impl std::fmt::Debug for RenewMachineToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RenewMachineToken([REDACTED])")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "result", rename_all = "camelCase", deny_unknown_fields)]
pub enum MachineTokenRenewal {
    Renewed {},
    Refused { reason: MachineTokenRenewalRefusal },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MachineTokenRenewalRefusal {
    TokenInvalid {},
    TokenExpired {},
    WrongClient {},
    WrongInstallation {},
    EnrollmentNotActive {},
}
