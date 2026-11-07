//! Step 0001: the Fleet database gains the update ledger before anything moves. Every
//! later step reads and writes this ledger, which stays in the file it describes.
use crate::{
    root_layout::RouterUpdateError,
    update_barrier::{UpdateBarrier, wait_at},
};
use message_board_storage::{RouterUpdateStep, UpdateLedger, UpdateStepProgress};
use std::path::Path;

/// Whether step 0001 migrates an existing Fleet database or creates a missing one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LedgerStoreOpen {
    Existing,
    Create,
}

pub(crate) async fn create_ledger(
    root: &Path,
    store: &Path,
    open: LedgerStoreOpen,
) -> Result<(), RouterUpdateError> {
    let storage_error = |source| RouterUpdateError::Store {
        store: store.to_owned(),
        source,
    };
    let ledger = match open {
        LedgerStoreOpen::Existing => UpdateLedger::migrate_existing(store)
            .await
            .map_err(storage_error)?,
        LedgerStoreOpen::Create => {
            let directory = codex_router_host::router_collaboration_directory(root);
            tokio::fs::create_dir_all(&directory)
                .await
                .map_err(|source| RouterUpdateError::File {
                    path: directory,
                    source,
                })?;
            UpdateLedger::create(store).await.map_err(storage_error)?
        }
    };
    let recorded = async {
        wait_at(UpdateBarrier::LedgerMigrated)
            .await
            .map_err(RouterUpdateError::Barrier)?;
        ledger
            .record_step(
                RouterUpdateStep::CreateLedger.number(),
                UpdateStepProgress::Done,
            )
            .await
            .map_err(storage_error)?;
        wait_at(UpdateBarrier::LedgerStepDone)
            .await
            .map_err(RouterUpdateError::Barrier)
    }
    .await;
    ledger.close().await;
    recorded
}
