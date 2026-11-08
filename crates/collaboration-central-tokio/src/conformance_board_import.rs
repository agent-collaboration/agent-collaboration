//! Test setup enters the actual Registry/Fleet/Copy owners without an HTTP fixture route.
use super::{Arc, NativeObjects, ObjectAddress, RuntimeFace, StoreError};
impl NativeObjects {
    pub(crate) async fn import_board_snapshot(
        self: &Arc<Self>,
        owner: collaboration_central_core::OwnerIdentity,
        state: collaboration_central_core::ConformanceBoardState,
        pages: Vec<fleet_protocol::SnapshotPage>,
        manifest: fleet_protocol::SnapshotManifest,
    ) -> Result<fleet_protocol::ReplicationPosition, StoreError> {
        let slot = self.slot(ObjectAddress::Fleet).await?;
        let runtime = slot
            .current_runtime()
            .ok_or_else(|| StoreError::backend("Registry missing"))?;
        match runtime.as_ref() {
            RuntimeFace::Fleet(kernel) => {
                kernel
                    .face()
                    .conformance_import_board_snapshot(owner, state, pages, manifest)
                    .await
            }
            RuntimeFace::Copy(_) => Err(StoreError::backend("not Fleet")),
        }
    }
}
