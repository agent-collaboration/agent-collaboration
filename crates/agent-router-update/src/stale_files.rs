//! Files found after the step that handled them was done. A done step never reads what
//! it already handled again: each such file is removed unread, under the Host lock,
//! before any pending step runs, and nothing it was carried into is touched.
//!
//! A file is stale only while what it was carried into exists. If that is gone, the
//! file may hold the only copy, so the update stops and names both for the owner.
use crate::{
    merge_step,
    root_layout::{RootLayout, RouterUpdateError, exists, legacy_source_path},
};
use codex_router_host::{router_collaboration_directory, router_dead_files, router_settings_path};
use codex_router_secret_store::account_credential_lock::account_credential_lock_directory;
use codex_router_state::{former_provider_proxy_database_path, provider_proxy_database_path};
use message_board_storage::{LegacyFleetSource, RouterUpdateStep, UpdateSourceProgress};
use std::path::{Path, PathBuf};

/// The older settings files step 0007 carries into `settings.jsonc`.
const CARRIED_SETTINGS_FILES: [&str; 2] = ["providers.json", "ports.json"];
/// The picker's machine list under its older name, and the name step 0007 gives it.
const LEGACY_MACHINE_LIST: &str = "routers.jsonc";
const MACHINE_LIST: &str = "agent-sessions.jsonc";

/// A file found after the step that handled it was done.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StaleFile {
    /// An older store step 0005 already carried and checked, with any log or
    /// shared-memory file beside it. These copies hold nothing the Fleet database lacks.
    MergedStore(PathBuf),
    /// A file step 0009 already removed, which nothing reads.
    DeadFile(PathBuf),
    /// An older settings file or machine list step 0007 already carried into a file that
    /// is still there.
    CarriedSettingsFile {
        path: PathBuf,
        carried_into: PathBuf,
    },
    /// The provider proxy store's database, log, shared-memory file or lock folder under
    /// its former name after step 0008 renamed the store, which is still there.
    RenamedProxyStore { path: PathBuf, renamed_to: PathBuf },
}

impl std::fmt::Display for StaleFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MergedStore(path) => write!(
                formatter,
                "stale store: remove {} unread; its rows were already moved",
                path.display()
            ),
            Self::DeadFile(path) => write!(
                formatter,
                "stale file: remove {} unread; nothing reads it",
                path.display()
            ),
            Self::CarriedSettingsFile { path, carried_into } => write!(
                formatter,
                "stale settings file: remove {} unread; {} already holds it",
                path.display(),
                carried_into.display()
            ),
            Self::RenamedProxyStore { path, renamed_to } => write!(
                formatter,
                "stale provider proxy store file: remove {} unread; {} already holds its accounts",
                path.display(),
                renamed_to.display()
            ),
        }
    }
}

/// Every stale file on `root`, given the ledger `layout` read.
pub(crate) fn stale_files(
    root: &Path,
    layout: &RootLayout,
) -> Result<Vec<StaleFile>, RouterUpdateError> {
    let step_done = |step: RouterUpdateStep| layout.ledger.step_done(step.number());
    let mut stale = Vec::new();
    // An older store is stale once step 0005 is done, or once the ledger records that
    // store itself removed: either way its rows already moved and were checked.
    let merge_done = step_done(RouterUpdateStep::MergeFleetStores);
    for source in LegacyFleetSource::ALL {
        let removed = layout.source_progress(source) == Some(UpdateSourceProgress::Removed);
        let path = legacy_source_path(root, source);
        if (merge_done || removed) && merge_step::any_store_file_present(&path)? {
            stale.push(StaleFile::MergedStore(path));
        }
    }
    if step_done(RouterUpdateStep::CarrySettingsFiles) {
        let settings = router_settings_path(root);
        for file in CARRIED_SETTINGS_FILES {
            push_if_carried(
                root.join(file),
                &settings,
                &mut stale,
                |path, carried_into| StaleFile::CarriedSettingsFile { path, carried_into },
            )?;
        }
        push_if_carried(
            root.join(LEGACY_MACHINE_LIST),
            &root.join(MACHINE_LIST),
            &mut stale,
            |path, carried_into| StaleFile::CarriedSettingsFile { path, carried_into },
        )?;
    }
    if step_done(RouterUpdateStep::RenameProviderProxyStore) {
        let former = former_provider_proxy_database_path(root);
        let current = provider_proxy_database_path(root);
        let mut former_files = vec![
            former.clone(),
            merge_step::side_file(&former, "-wal"),
            merge_step::side_file(&former, "-shm"),
        ];
        former_files.extend(account_credential_lock_directory(&former));
        for path in former_files {
            push_if_carried(path, &current, &mut stale, |path, renamed_to| {
                StaleFile::RenamedProxyStore { path, renamed_to }
            })?;
        }
    }
    if step_done(RouterUpdateStep::RemoveDeadFiles) {
        let dead = router_dead_files(root).map_err(|source| RouterUpdateError::File {
            path: router_collaboration_directory(root),
            source,
        })?;
        stale.extend(dead.into_iter().map(StaleFile::DeadFile));
    }
    Ok(stale)
}

/// Lists `path` as stale when it is present and `carried_into` holds what it carried;
/// present without `carried_into` stops the update, naming both.
fn push_if_carried(
    path: PathBuf,
    carried_into: &Path,
    stale: &mut Vec<StaleFile>,
    stale_file: impl FnOnce(PathBuf, PathBuf) -> StaleFile,
) -> Result<(), RouterUpdateError> {
    if !exists(&path)? {
        return Ok(());
    }
    if !exists(carried_into)? {
        return Err(RouterUpdateError::StaleSourceWithoutDestination {
            path,
            destination: carried_into.to_owned(),
        });
    }
    stale.push(stale_file(path, carried_into.to_owned()));
    Ok(())
}

/// Removes one stale file, and a merged store's side files, without opening it.
pub(crate) async fn remove_stale_file(stale: &StaleFile) -> Result<(), RouterUpdateError> {
    match stale {
        StaleFile::MergedStore(path) => merge_step::remove_stale_source(path).await,
        StaleFile::DeadFile(path) | StaleFile::CarriedSettingsFile { path, .. } => {
            merge_step::remove_if_present(path).await
        }
        StaleFile::RenamedProxyStore { path, .. } => remove_file_or_folder(path).await,
    }
}

/// The proxy store's lock folder is a folder; its other former files are files.
async fn remove_file_or_folder(path: &Path) -> Result<(), RouterUpdateError> {
    let removed = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_dir() => tokio::fs::remove_dir_all(path).await,
        Ok(_) => return merge_step::remove_if_present(path).await,
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
