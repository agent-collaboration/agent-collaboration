//! Translate control notice variants to their typed storage columns.
use crate::fleet_record_codec::{check_private_id, corrupt, required};
use board_sync_protocol::BoardNotice;
use durable_steps::StoreError;
use fleet_sync_protocol::{DeliveryStream, Epoch};

pub(super) struct NoticeColumns {
    pub notice: BoardNotice,
    pub kind: &'static str,
    pub board: Option<String>,
    pub project: Option<String>,
    pub epoch: Option<String>,
    pub feed_through: Option<String>,
}

impl NoticeColumns {
    pub fn from_notice(notice: BoardNotice) -> Self {
        let (kind, board, project, epoch, feed_through) = match &notice {
            BoardNotice::FleetFeedAdvanced { through } => (
                "fleet_feed_advanced",
                None,
                None,
                None,
                Some(through.get().to_string()),
            ),
            BoardNotice::BoardArchiving { board } => (
                "board_archiving",
                Some(board.as_str().to_owned()),
                None,
                None,
                None,
            ),
            BoardNotice::LeaseGranted { project, epoch } => (
                "lease_granted",
                None,
                Some(project.as_str().to_owned()),
                Some(epoch.value().to_string()),
                None,
            ),
            BoardNotice::LeaseSuperseded { project, epoch } => (
                "lease_superseded",
                None,
                Some(project.as_str().to_owned()),
                Some(epoch.value().to_string()),
                None,
            ),
            // The intermediate C1–C3 batch retains the old mail chain until C4.
            BoardNotice::MailWaiting {
                stream: DeliveryStream::Items,
            } => ("mail_waiting_items", None, None, None, None),
            BoardNotice::MailWaiting {
                stream: DeliveryStream::Controls,
            } => ("mail_waiting_controls", None, None, None, None),
        };
        Self {
            notice,
            kind,
            board,
            project,
            epoch,
            feed_through,
        }
    }
}

pub(super) fn decode_notice(
    kind: Option<String>,
    board: Option<String>,
    project: Option<String>,
    epoch: Option<String>,
    feed_through: Option<String>,
) -> Result<BoardNotice, StoreError> {
    match (
        required(kind, "machine_control_notices.kind")?.as_str(),
        board,
        project,
        epoch,
        feed_through,
    ) {
        ("fleet_feed_advanced", None, None, None, Some(through)) => {
            Ok(BoardNotice::FleetFeedAdvanced {
                through: message_board::FleetPosition::try_from(canonical_counter(&through)?)
                    .map_err(|error| corrupt("machine_control_notices.feed_through", error))?,
            })
        }
        ("board_archiving", Some(board), None, None, None) => {
            check_private_id(&board, "machine_control_notices.board_id")?;
            Ok(BoardNotice::BoardArchiving {
                board: board
                    .try_into()
                    .map_err(|error| corrupt("machine_control_notices.board_id", error))?,
            })
        }
        (tag @ ("lease_granted" | "lease_superseded"), None, Some(project), Some(epoch), None) => {
            check_private_id(&project, "machine_control_notices.project_id")?;
            let project = project
                .try_into()
                .map_err(|error| corrupt("machine_control_notices.project_id", error))?;
            let epoch = Epoch::new(canonical_counter(&epoch)?)
                .map_err(|error| corrupt("machine_control_notices.epoch", error))?;
            Ok(if tag == "lease_granted" {
                BoardNotice::LeaseGranted { project, epoch }
            } else {
                BoardNotice::LeaseSuperseded { project, epoch }
            })
        }
        ("mail_waiting_items", None, None, None, None) => Ok(BoardNotice::MailWaiting {
            stream: DeliveryStream::Items,
        }),
        ("mail_waiting_controls", None, None, None, None) => Ok(BoardNotice::MailWaiting {
            stream: DeliveryStream::Controls,
        }),
        _ => Err(corrupt(
            "machine_control_notices.kind",
            "unknown kind or mismatched variant columns",
        )),
    }
}

fn canonical_counter(value: &str) -> Result<u64, StoreError> {
    let counter = value
        .parse::<u64>()
        .map_err(|error| corrupt("machine_control_notices.counter", error))?;
    if counter.to_string() != value || i64::try_from(counter).is_err() {
        return Err(corrupt(
            "machine_control_notices.counter",
            "noncanonical SQLite integer",
        ));
    }
    Ok(counter)
}

#[cfg(test)]
#[path = "machine_notice_columns_test.rs"]
mod tests;
