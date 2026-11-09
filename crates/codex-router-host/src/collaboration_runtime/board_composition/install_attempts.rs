//! The existing cache transaction selects one attempt per granted project epoch.
use fleet_sync_protocol::{Epoch, InstallAttemptId};
use message_board::ProjectId;
use message_board_storage::FleetCache;
use std::io;

pub(super) async fn recorded_attempt(
    cache: &FleetCache,
    project: &ProjectId,
    epoch: Epoch,
) -> io::Result<InstallAttemptId> {
    cache
        .snapshot_install_attempt(project, epoch)
        .await
        .map_err(io::Error::other)
}

#[cfg(test)]
#[path = "install_attempts_test.rs"]
mod tests;
