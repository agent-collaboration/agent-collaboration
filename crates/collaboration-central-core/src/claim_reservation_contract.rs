//! The Fleet-owned settlement state used by the shared lease gate.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ClaimReservationState {
    Active,
    Settled { outcome: ClaimReservationOutcome },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClaimReservationOutcome {
    Granted,
    Refused,
}
