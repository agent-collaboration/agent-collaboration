use chrono::{DateTime, Utc};
use message_board::{FleetPosition, RecordRef};
use serde::{Deserialize, Serialize};

use crate::BoardPayload;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetFeedRead {
    Page {
        records: Vec<BoardPayload>,
        through: FleetPosition,
        head: FleetPosition,
        as_of: DateTime<Utc>,
    },
    CursorExpired {
        floor: FleetPosition,
    },
    CursorInvalid,
}

/// Continues a full fleet read at its original cut, after a stable record identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetSnapshotCursor {
    pub snapshot_at: FleetPosition,
    pub after: RecordRef,
}
