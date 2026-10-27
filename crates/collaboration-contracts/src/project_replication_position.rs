//! The view-facing operation of PD1's ProjectReplication port, bound by M4.
use message_board::{BoardError, EpochNumber, FeedCursor, FeedPosition, ProjectId};
use std::{future::Future, pin::Pin};

pub type ReplicatedPositionFuture<'port> =
    Pin<Box<dyn Future<Output = Result<FeedCursor, BoardError>> + Send + 'port>>;
pub type AcknowledgedMailChangesFuture<'port> = Pin<
    Box<
        dyn Future<Output = Result<message_board_storage::AcknowledgedProjectChanges, BoardError>>
            + Send
            + 'port,
    >,
>;

pub trait ProjectReplicationPosition: Send + Sync {
    fn confirmed_writer_epoch<'port>(
        &'port self,
        _project: &'port ProjectId,
    ) -> Pin<Box<dyn Future<Output = Option<fleet_sync_protocol::Epoch>> + Send + 'port>> {
        Box::pin(async { None })
    }
    fn acknowledged_mail_changes<'port>(
        &'port self,
        project: &'port ProjectId,
    ) -> AcknowledgedMailChangesFuture<'port>;
    fn replicated_through<'port>(
        &'port self,
        project: &'port ProjectId,
        epoch: EpochNumber,
    ) -> ReplicatedPositionFuture<'port>;
}

/// Open M2 stand-in: nothing has been acknowledged until the real M4 port is installed.
pub struct UnacknowledgedProjectReplication;
impl ProjectReplicationPosition for UnacknowledgedProjectReplication {
    fn acknowledged_mail_changes<'port>(
        &'port self,
        _project: &'port ProjectId,
    ) -> AcknowledgedMailChangesFuture<'port> {
        Box::pin(async { Ok(message_board_storage::AcknowledgedProjectChanges::default()) })
    }
    fn replicated_through<'port>(
        &'port self,
        _project: &'port ProjectId,
        epoch: EpochNumber,
    ) -> ReplicatedPositionFuture<'port> {
        Box::pin(async move {
            Ok(FeedCursor {
                epoch,
                position: FeedPosition::ZERO,
            })
        })
    }
}
