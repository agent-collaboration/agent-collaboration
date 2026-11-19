//! Stable database identity for Host tests that prepare local session records.
use std::{io, path::Path};
use workspace_ids::MachineId;

pub(crate) async fn machine_id(directory: &Path) -> io::Result<MachineId> {
    collaboration_runtime::load_or_create_machine_id(
        directory,
        runtime_primitives::SyncTime::from_unix_millis(0),
    )
    .await
    .map_err(io::Error::other)
}

/// The old isolated board fixtures still take the id in their PeerId type.
pub(crate) async fn board_peer(directory: &Path) -> io::Result<fleet_sync_protocol::PeerId> {
    fleet_sync_protocol::PeerId::try_from(machine_id(directory).await?.as_str().to_owned())
        .map_err(io::Error::other)
}
