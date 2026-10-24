//! Project feed and handoff mappings use the library's capture-off scope.
use fleet_sync_writer::{
    BookkeepingError, CaptureBookkeeping, CapturedTransaction, MaintenanceTransaction,
};

pub struct BoardCaptureBookkeeping;

impl CaptureBookkeeping for BoardCaptureBookkeeping {
    async fn record(
        &self,
        scope: &mut MaintenanceTransaction<'_>,
        captured: &CapturedTransaction<'_>,
    ) -> Result<(), BookkeepingError> {
        message_board_storage::record_project_capture(scope, captured)
            .await
            .map_err(|error| BookkeepingError {
                message: error.to_string(),
            })
    }
}
