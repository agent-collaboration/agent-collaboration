//! Test setup enters the actual Registry/Fleet/Copy owners without an HTTP
//! fixture route, as an owner call event on the Fleet object: what it owes is
//! sent after that event, like any other call's.
use super::{Arc, NativeObjects, StoreError};
use collaboration_central_core::conformance_setup_calls::{
    ConformanceImportBoardSnapshot, ImportBoardSnapshotRequest, encode_conformance_setup,
};
impl NativeObjects {
    pub(crate) async fn import_board_snapshot(
        self: &Arc<Self>,
        owner: collaboration_central_core::OwnerIdentity,
        state: collaboration_central_core::ConformanceBoardState,
        pages: Vec<fleet_protocol::SnapshotPage>,
        manifest: fleet_protocol::SnapshotManifest,
    ) -> Result<fleet_protocol::ReplicationPosition, StoreError> {
        let identity = durable_steps::CallId {
            operation: durable_steps::OperationId::new(durable_steps::Uuid::now_v7())
                .map_err(StoreError::backend)?,
            step: durable_steps::StepName::new("conformance_setup").map_err(StoreError::backend)?,
        };
        let call = encode_conformance_setup(
            owner,
            ConformanceImportBoardSnapshot,
            ImportBoardSnapshotRequest {
                board_state: state,
                pages,
                manifest,
            },
            identity,
        )
        .map_err(|failure| StoreError::backend(format!("board import encoding: {failure:?}")))?;
        let (address, body) = call.into_parts();
        let answer = self
            .call(address, body)
            .await
            .map_err(|failure| StoreError::backend(format!("board import call: {failure:?}")))?;
        serde_json::from_slice(&answer).map_err(StoreError::backend)
    }
}
