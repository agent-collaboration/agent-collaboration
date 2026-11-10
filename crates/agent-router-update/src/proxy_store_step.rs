//! Step 0008: the provider proxy store is renamed from `state.sqlite` to
//! `provider-proxy.sqlite`, with its account lock folder.
//!
//! The store is opened as it is (never created or migrated), every committed page moves
//! from its write-ahead log into the database file, and the connection closes. Only
//! when no log or shared-memory file is left does the file change name, so no committed
//! account, quota or routing row stays in a log the new name doesn't own. The lock
//! folder (`<database>.credential-locks`) follows. A rerun continues from the files it
//! finds; both names present for the store or for its folder stops the update and names
//! both. The proxy itself is unchanged: it opens whatever path it is given.
use crate::{
    merge_step::side_file,
    rename_step::refuse_leftover_log,
    root_layout::{RootLayout, RouterUpdateError, exists},
    update_barrier::{UpdateBarrier, wait_at},
};
use codex_router_secret_store::account_credential_lock::account_credential_lock_directory;
use codex_router_state::{
    checkpoint_provider_proxy_database, former_provider_proxy_database_path,
    provider_proxy_database_path,
};
use message_board_storage::{RouterUpdateStep, UpdateLedger, UpdateStepProgress};
use std::path::{Path, PathBuf};

/// What step 0008 will do on a root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyStoreChange {
    /// The store to checkpoint and rename, when it is still under its former name.
    pub store: Option<(PathBuf, PathBuf)>,
    /// The lock folder to rename, when it is still under its former name.
    pub lock_folder: Option<(PathBuf, PathBuf)>,
}

impl std::fmt::Display for ProxyStoreChange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let renames: Vec<String> = [&self.store, &self.lock_folder]
            .into_iter()
            .flatten()
            .map(|(from, to)| format!("rename {} to {}", from.display(), to.display()))
            .collect();
        if renames.is_empty() {
            return write!(formatter, "no provider proxy store under its former name");
        }
        if self.store.is_some() {
            write!(formatter, "checkpoint and ")?;
        }
        write!(formatter, "{}", renames.join(", then "))
    }
}

/// Decides what step 0008 does, reading only which files exist.
pub(crate) fn plan_proxy_store_step(root: &Path) -> Result<ProxyStoreChange, RouterUpdateError> {
    let former = former_provider_proxy_database_path(root);
    let current = provider_proxy_database_path(root);
    let store = rename_pair(former.clone(), current.clone())?;
    // A log or shared-memory file without its database under the former name could
    // hold committed rows no store owns now; the owner settles it.
    if store.is_none()
        && !exists(&former)?
        && (exists(&side_file(&former, "-wal"))? || exists(&side_file(&former, "-shm"))?)
    {
        return Err(RouterUpdateError::LogRemainsAfterCheckpoint { path: former });
    }
    let lock_folder = match (
        account_credential_lock_directory(&former),
        account_credential_lock_directory(&current),
    ) {
        (Some(former_folder), Some(current_folder)) => rename_pair(former_folder, current_folder)?,
        _ => None,
    };
    Ok(ProxyStoreChange { store, lock_folder })
}

/// `Some` when only the former name exists; both names is a conflict the owner settles.
fn rename_pair(
    former: PathBuf,
    current: PathBuf,
) -> Result<Option<(PathBuf, PathBuf)>, RouterUpdateError> {
    match (exists(&former)?, exists(&current)?) {
        (true, true) => Err(RouterUpdateError::BothProviderProxyNames { current, former }),
        (true, false) => Ok(Some((former, current))),
        (false, _) => Ok(None),
    }
}

/// Runs step 0008 as planned under the Host lock.
pub(crate) async fn rename_provider_proxy_store(root: &Path) -> Result<(), RouterUpdateError> {
    let change = plan_proxy_store_step(root)?;
    if let Some((former, current)) = &change.store {
        checkpoint_provider_proxy_database(former)
            .await
            .map_err(|source| RouterUpdateError::ProviderProxyStore {
                store: former.clone(),
                source,
            })?;
        refuse_leftover_log(former)?;
        barrier(UpdateBarrier::ProxyStoreCheckpointed).await?;
        rename(former, current)?;
        barrier(UpdateBarrier::ProxyStoreRenamed).await?;
    }
    if let Some((former, current)) = &change.lock_folder {
        rename(former, current)?;
        barrier(UpdateBarrier::ProxyLockFolderRenamed).await?;
    }
    let layout = RootLayout::read(root).await?;
    let ledger = UpdateLedger::migrate_existing(&layout.store)
        .await
        .map_err(|source| RouterUpdateError::Store {
            store: layout.store.clone(),
            source,
        })?;
    let recorded = ledger
        .record_step(
            RouterUpdateStep::RenameProviderProxyStore.number(),
            UpdateStepProgress::Done,
        )
        .await
        .map_err(|source| RouterUpdateError::Store {
            store: layout.store.clone(),
            source,
        });
    ledger.close().await;
    recorded?;
    barrier(UpdateBarrier::ProxyStoreDone).await
}

fn rename(from: &Path, to: &Path) -> Result<(), RouterUpdateError> {
    std::fs::rename(from, to).map_err(|source| RouterUpdateError::File {
        path: from.to_owned(),
        source,
    })
}

async fn barrier(point: UpdateBarrier) -> Result<(), RouterUpdateError> {
    wait_at(point).await.map_err(RouterUpdateError::Barrier)
}
