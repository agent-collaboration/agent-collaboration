//! Step 0005: the Fleet outbox and the board cursor key move out of their own files into
//! the Fleet database. For each source: its rows and its `written` ledger row commit in
//! one transaction; every value is compared; the database file is removed, then its side
//! files; then the source is recorded `removed`. A rerun continues from the ledger.
use crate::{
    router_update::{RootLayout, RouterUpdateError, exists, legacy_source_path},
    update_barrier::{UpdateBarrier, wait_at},
};
use message_board_storage::{
    LegacyFleetSource, RouterUpdateStep, UpdateLedger, UpdateSourceProgress, UpdateStepProgress,
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(crate) async fn merge_fleet_stores(root: &Path) -> Result<(), RouterUpdateError> {
    let layout = RootLayout::read(root).await?;
    let ledger = UpdateLedger::migrate_existing(&layout.store)
        .await
        .map_err(|source| RouterUpdateError::Store {
            store: layout.store.clone(),
            source,
        })?;
    let merged = async {
        for source in LegacyFleetSource::ALL {
            let path = legacy_source_path(root, source);
            match (layout.source_progress(source), exists(&path)?) {
                (None, false) | (Some(UpdateSourceProgress::Removed), false) => {}
                (None, true) => {
                    ledger
                        .carry_legacy_source(source, &path)
                        .await
                        .map_err(|error| source_error(&path, error))?;
                    barrier(UpdateBarrier::SourceWritten(source)).await?;
                    remove_carried_source(&ledger, source, &path).await?;
                }
                (Some(UpdateSourceProgress::Written), true) => {
                    remove_carried_source(&ledger, source, &path).await?;
                }
                (Some(UpdateSourceProgress::Written), false) => {
                    // Killed after the database file went: only side files can remain.
                    finish_source_removal(&ledger, source, &path).await?;
                }
                (Some(UpdateSourceProgress::Removed), true) => {
                    return Err(RouterUpdateError::RemovedSourceReappeared { path });
                }
            }
        }
        ledger
            .record_step(
                RouterUpdateStep::MergeFleetStores.number(),
                UpdateStepProgress::Done,
            )
            .await
            .map_err(|error| source_error(&layout.store, error))?;
        barrier(UpdateBarrier::MergeDone).await
    }
    .await;
    ledger.close().await;
    merged
}

/// Removes a source only once every one of its values is in the Fleet database.
async fn remove_carried_source(
    ledger: &UpdateLedger,
    source: LegacyFleetSource,
    path: &Path,
) -> Result<(), RouterUpdateError> {
    ledger
        .verify_legacy_source_carried(source, path)
        .await
        .map_err(|error| source_error(path, error))?;
    barrier(UpdateBarrier::SourceVerified(source)).await?;
    remove_if_present(path).await?;
    barrier(UpdateBarrier::SourceDatabaseRemoved(source)).await?;
    finish_source_removal(ledger, source, path).await
}

async fn finish_source_removal(
    ledger: &UpdateLedger,
    source: LegacyFleetSource,
    path: &Path,
) -> Result<(), RouterUpdateError> {
    for side_file in [side_file(path, "-wal"), side_file(path, "-shm")] {
        remove_if_present(&side_file).await?;
    }
    ledger
        .record_source(source, UpdateSourceProgress::Removed)
        .await
        .map_err(|error| source_error(path, error))?;
    barrier(UpdateBarrier::SourceRemoved(source)).await
}

/// A store's write-ahead log or shared-memory file, named as SQLite and Turso name them.
pub(crate) fn side_file(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

async fn remove_if_present(path: &Path) -> Result<(), RouterUpdateError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(RouterUpdateError::File {
            path: path.to_owned(),
            source,
        }),
    }
}

fn source_error(
    path: &Path,
    source: message_board_storage::RouterLocalStorageError,
) -> RouterUpdateError {
    RouterUpdateError::Store {
        store: path.to_owned(),
        source,
    }
}

async fn barrier(point: UpdateBarrier) -> Result<(), RouterUpdateError> {
    wait_at(point).await.map_err(RouterUpdateError::Barrier)
}
