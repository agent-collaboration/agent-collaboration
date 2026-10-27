//! Test setup enters the actual Registry/Fleet/Copy owners without an HTTP fixture route.
use super::{Arc, NativeObjects, ObjectAddress, RuntimeFace, StoreError};
impl NativeObjects {
    pub(crate) async fn import_board_snapshot(
        self: &Arc<Self>,
        owner: collaboration_central_core::OwnerIdentity,
        state: collaboration_central_core::ConformanceBoardState,
        pages: Vec<fleet_sync_protocol::SnapshotPage>,
        manifest: fleet_sync_protocol::SnapshotManifest,
    ) -> Result<fleet_sync_protocol::ReplicationPosition, StoreError> {
        let slot = self.slot(ObjectAddress::Registry).await?;
        let runtime = slot
            .runtime
            .get()
            .ok_or_else(|| StoreError::backend("Registry missing"))?;
        match runtime.as_ref() {
            RuntimeFace::Registry(runtime) => {
                runtime
                    .conformance_import_board_snapshot(owner, state, pages, manifest)
                    .await
            }
            _ => Err(StoreError::backend("not Registry")),
        }
    }
}
