//! Renewing a machine token on the open link (owner-approved fleet-access spec
//! R40/V32, 2026-11-04, and B3's renewal paragraph): "On the open connection,
//! the Router sends the fresh token (`RenewMachineToken`), and central answers
//! `renewed` or `refused { reason }`."
use serde::{Deserialize, Serialize};
use workspace_model::Timestamp;

/// A sign-in-issued machine token. It is a credential: `Debug` never shows it
/// and nothing logs or echoes it.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MachineTokenText(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("a machine token is one non-empty compact JWT without whitespace")]
pub struct InvalidMachineToken;

impl MachineTokenText {
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for MachineTokenText {
    type Error = InvalidMachineToken;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(InvalidMachineToken);
        }
        Ok(Self(value))
    }
}

impl From<MachineTokenText> for String {
    fn from(value: MachineTokenText) -> Self {
        value.0
    }
}

impl std::fmt::Debug for MachineTokenText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MachineTokenText(<redacted>)")
    }
}

/// Central's answer to one renewal on the open connection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TokenRenewalResult {
    /// The connection now counts until this instant: the new token's
    /// min(exp + 60 s, grant_expires_at). It may be later than before.
    Renewed { effective_expires_at: Timestamp },
    /// The old expiry stands; the socket stays open until it refuses a frame.
    Refused { reason: TokenRenewalRefusal },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenRenewalRefusal {
    /// The token failed sign-in verification.
    TokenRejected,
    /// The enrollment it names is no longer the machine's active one.
    EnrollmentNotActive,
    /// The token names another machine than this connection's.
    DifferentMachine,
    /// The token names another enrollment or client than this connection's.
    DifferentEnrollment,
    /// Central could not decide now; the old expiry stands.
    CentralUnavailable,
}

#[cfg(test)]
#[path = "link_renewal_test.rs"]
mod tests;
