//! Step 0010: `fleet-cache.sqlite` becomes `fleet.sqlite`. The ledger records the step
//! in progress, every page moves from the write-ahead log into the database file, and
//! only then does the file change name, so no committed row stays behind in a log the
//! new name doesn't own. A rerun that finds the new name only records the step done.
use crate::{
    merge_step::side_file,
    root_layout::{RootLayout, RouterUpdateError, exists},
    update_barrier::{UpdateBarrier, wait_at},
};
use codex_router_host::router_fleet_database;
use message_board_storage::{RouterUpdateStep, UpdateLedger, UpdateStepProgress};
use std::path::Path;

pub(crate) async fn rename_fleet_database(root: &Path) -> Result<(), RouterUpdateError> {
    let layout = RootLayout::read(root).await?;
    let current = router_fleet_database(root);
    if !layout.store_is_current {
        let former = layout.store;
        let ledger = UpdateLedger::migrate_existing(&former)
            .await
            .map_err(|source| store_error(&former, source))?;
        let started = async {
            ledger
                .record_step(
                    RouterUpdateStep::RenameFleetDatabase.number(),
                    UpdateStepProgress::InProgress,
                )
                .await
                .map_err(|source| store_error(&former, source))?;
            barrier(UpdateBarrier::RenameBeforeCheckpoint).await
        }
        .await;
        if let Err(error) = started {
            ledger.close().await;
            return Err(error);
        }
        ledger
            .checkpoint_and_close()
            .await
            .map_err(|source| store_error(&former, source))?;
        refuse_leftover_log(&former)?;
        barrier(UpdateBarrier::RenameCheckpointed).await?;
        std::fs::rename(&former, &current).map_err(|source| RouterUpdateError::File {
            path: former.clone(),
            source,
        })?;
        barrier(UpdateBarrier::Renamed).await?;
    }
    let ledger = UpdateLedger::migrate_existing(&current)
        .await
        .map_err(|source| store_error(&current, source))?;
    let recorded = ledger
        .record_step(
            RouterUpdateStep::RenameFleetDatabase.number(),
            UpdateStepProgress::Done,
        )
        .await
        .map_err(|source| store_error(&current, source));
    ledger.close().await;
    recorded?;
    barrier(UpdateBarrier::RenameDone).await
}

/// After the checkpoint the write-ahead log is empty (removed here) and no shared-memory
/// file exists; anything else means a row could be left behind, so the rename stops.
pub(crate) fn refuse_leftover_log(store: &Path) -> Result<(), RouterUpdateError> {
    let log = side_file(store, "-wal");
    if exists(&log)? {
        let length = std::fs::metadata(&log)
            .map_err(|source| RouterUpdateError::File {
                path: log.clone(),
                source,
            })?
            .len();
        if length > 0 {
            return Err(RouterUpdateError::LogRemainsAfterCheckpoint {
                path: store.to_owned(),
            });
        }
        std::fs::remove_file(&log).map_err(|source| RouterUpdateError::File {
            path: log.clone(),
            source,
        })?;
    }
    if exists(&side_file(store, "-shm"))? {
        return Err(RouterUpdateError::LogRemainsAfterCheckpoint {
            path: store.to_owned(),
        });
    }
    Ok(())
}

fn store_error(
    store: &Path,
    source: message_board_storage::RouterLocalStorageError,
) -> RouterUpdateError {
    RouterUpdateError::Store {
        store: store.to_owned(),
        source,
    }
}

async fn barrier(point: UpdateBarrier) -> Result<(), RouterUpdateError> {
    wait_at(point).await.map_err(RouterUpdateError::Barrier)
}
