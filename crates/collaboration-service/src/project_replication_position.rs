//! The view-facing operation of PD1's ProjectReplication port, bound by M4.
use message_board::{EpochNumber, FeedCursor, FeedPosition, ProjectId};
use std::{future::Future, pin::Pin};

pub type ReplicatedPositionFuture<'port> = Pin<Box<dyn Future<Output = FeedCursor> + Send + 'port>>;

pub trait ProjectReplicationPosition: Send + Sync {
    fn replicated_through<'port>(
        &'port self,
        project: &'port ProjectId,
        epoch: EpochNumber,
    ) -> ReplicatedPositionFuture<'port>;
}

/// Open M2 stand-in: nothing has been acknowledged until the real M4 port is installed.
pub struct UnacknowledgedProjectReplication;
impl ProjectReplicationPosition for UnacknowledgedProjectReplication {
    fn replicated_through<'port>(
        &'port self,
        _project: &'port ProjectId,
        epoch: EpochNumber,
    ) -> ReplicatedPositionFuture<'port> {
        Box::pin(async move {
            FeedCursor {
                epoch,
                position: FeedPosition::ZERO,
            }
        })
    }
}
