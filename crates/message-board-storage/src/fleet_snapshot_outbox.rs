//! Outbox reads a Fleet read snapshot makes on the connection it already holds. The
//! cache and the outbox share the Fleet database's one connection, so a caller holding a
//! snapshot reads pending creations here, never through the outbox view, which would
//! wait for the connection the snapshot holds.
use crate::{FleetCacheSnapshot, RouterLocalStorageError};

impl FleetCacheSnapshot {
    /// Projects still pending creation in `workspace`, consistent with this snapshot.
    pub async fn pending_project_ids(
        &mut self,
        workspace: &message_board::WorkspaceId,
    ) -> Result<Vec<message_board::ProjectId>, RouterLocalStorageError> {
        crate::fleet_outbox::pending_project_ids_on(&mut *self.transaction, workspace).await
    }
}
