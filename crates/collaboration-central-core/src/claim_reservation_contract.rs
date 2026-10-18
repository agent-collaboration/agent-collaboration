//! Fleet's conservative archive blocker, distinct from Registry's lease.
use collaboration_replication_protocol::{ProjectId, RequestId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimReservation {
    pub project: ProjectId,
    pub attempt: RequestId,
    pub state: ClaimReservationState,
}

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ReserveClaimResult {
    Reserved,
    AlreadySettled { outcome: ClaimReservationOutcome },
    Refused(ClaimReservationRefusal),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClaimReservationRefusal {
    BoardArchiving,
    BoardArchived,
    NotFound,
}
