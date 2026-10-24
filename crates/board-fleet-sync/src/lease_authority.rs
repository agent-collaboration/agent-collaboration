//! Status reads observe the library owner outside admission's write scope.
use fleet_sync_lease::WriterState;
use message_board::ProjectId;
use std::future::Future;

pub trait LeaseAuthority: Send + Sync {
    type Error: std::error::Error + Send + Sync;

    fn writer_state(
        &self,
        project: &ProjectId,
    ) -> impl Future<Output = Result<WriterState, Self::Error>> + Send;
}
