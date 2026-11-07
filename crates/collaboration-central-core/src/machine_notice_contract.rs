//! Durable control work distinguishes a new append from a hint for an existing row.
use board_sync_protocol::{BoardNotice, PositionedBoardNotice};
use fleet_protocol::PeerId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum MachineNoticeAppend {
    Append {
        peer: PeerId,
        notice: BoardNotice,
    },
    Hint {
        peer: PeerId,
        notice: PositionedBoardNotice,
    },
}
