//! Step 0009: files nothing reads any more are removed. Approval routes an older Router
//! kept in JSON are imported into `interaction.sqlite` first, then their file goes; the
//! enrollment reset journal goes once its reset is finished; leftover native schema
//! export folders go. Every removal tolerates a file already gone, so a rerun after a
//! crash finishes the same list.
use crate::{
    merge_step::remove_if_present,
    root_layout::{RootLayout, RouterUpdateError},
    update_barrier::{UpdateBarrier, wait_at},
};
use codex_router_host::{
    LEGACY_APPROVAL_ROUTES_FILE, completed_enrollment_reset_journal,
    router_collaboration_directory, router_dead_files, router_schema_export_folders,
};
use message_board_storage::{RouterUpdateStep, UpdateLedger, UpdateStepProgress};
use std::path::{Path, PathBuf};

/// What step 0009 would do to `root` now.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DeadFileRemoval {
    pub(crate) import_routes: Option<PathBuf>,
    pub(crate) remove: Vec<PathBuf>,
}

/// Lists the dead files, the journal of a finished reset and leftover export folders.
/// A reset step 0002 is still to finish counts as finished, since it runs first.
pub(crate) fn dead_file_removal(
    root: &Path,
    reset_still_to_finish: bool,
) -> Result<DeadFileRemoval, RouterUpdateError> {
    let directory = router_collaboration_directory(root);
    let file_error = |source| RouterUpdateError::File {
        path: directory.clone(),
        source,
    };
    let routes = directory.join(LEGACY_APPROVAL_ROUTES_FILE);
    let mut remove = router_dead_files(root).map_err(file_error)?;
    let import_routes = remove.contains(&routes).then_some(routes);
    let journal = crate::reset_step::enrollment_reset_journal(root);
    let journal_finished = completed_enrollment_reset_journal(root)
        .map_err(|source| RouterUpdateError::File {
            path: journal.clone(),
            source,
        })?
        .is_some();
    if journal_finished || (reset_still_to_finish && journal.exists()) {
        remove.push(journal);
    }
    remove.extend(router_schema_export_folders(root).map_err(file_error)?);
    remove.sort();
    Ok(DeadFileRemoval {
        import_routes,
        remove,
    })
}

pub(crate) async fn remove_dead_files(root: &Path) -> Result<(), RouterUpdateError> {
    let layout = RootLayout::read(root).await?;
    // Step 0002 is done by now, so only a finished journal is listed.
    let removal = dead_file_removal(root, false)?;
    if let Some(routes) = &removal.import_routes {
        import_approval_routes(root, routes).await?;
    }
    for path in &removal.remove {
        remove_path(path).await?;
    }
    let store_error = |source| RouterUpdateError::Store {
        store: layout.store.clone(),
        source,
    };
    let ledger = UpdateLedger::migrate_existing(&layout.store)
        .await
        .map_err(store_error)?;
    let recorded = ledger
        .record_step(
            RouterUpdateStep::RemoveDeadFiles.number(),
            UpdateStepProgress::Done,
        )
        .await
        .map_err(store_error);
    ledger.close().await;
    recorded?;
    wait_at(UpdateBarrier::DeadFilesDone)
        .await
        .map_err(RouterUpdateError::Barrier)
}

/// Imports the routes into `interaction.sqlite` through the broker that owns them, which
/// removes the file once they are committed; importing twice keeps the first copy.
async fn import_approval_routes(root: &Path, routes: &Path) -> Result<(), RouterUpdateError> {
    let import_error = |source: Box<dyn std::error::Error + Send + Sync>| {
        RouterUpdateError::ApprovalRoutesImport {
            path: routes.to_owned(),
            source,
        }
    };
    let store = interaction_storage::InteractionStore::open(
        router_collaboration_directory(root).join("interaction.sqlite"),
    )
    .await
    .map_err(|error| import_error(Box::new(error)))?;
    let imported = collaboration_runtime::ServiceInteractionBroker::import_legacy_approval_routes(
        &store,
        routes,
        chrono::Utc::now().timestamp_millis(),
    )
    .await;
    let closed = store.close().await;
    imported.map_err(|error| import_error(Box::new(error)))?;
    closed.map_err(|error| import_error(Box::new(error)))
}

async fn remove_path(path: &Path) -> Result<(), RouterUpdateError> {
    let removed = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_dir() => tokio::fs::remove_dir_all(path).await,
        Ok(_) => return remove_if_present(path).await,
        Err(error) => Err(error),
    };
    match removed {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(RouterUpdateError::File {
            path: path.to_owned(),
            source,
        }),
    }
}
