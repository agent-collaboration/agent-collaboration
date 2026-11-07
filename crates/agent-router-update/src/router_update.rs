//! Runs the one-time steps a Router root still needs, in order, with its Host stopped.
//!
//! The step catalog (`RouterUpdateStep`) is the one the Host's start-up guard reads,
//! so the steps run here are exactly the ones it refuses on until they are done.
use crate::{
    ledger_step::{self, LedgerStoreOpen},
    merge_step, rename_step,
};
use codex_router_host::{
    HostCoordinationPaths, HostInstanceLock, InstanceAcquireError, router_collaboration_directory,
    router_fleet_cache_database, router_fleet_database,
};
use message_board_storage::{
    LegacyFleetSource, RouterLocalStorageError, RouterUpdateStep, StoredUpdateLedger,
    UpdateLedgerEntry, UpdateSourceProgress, read_update_ledger,
};
use std::path::{Path, PathBuf};

/// One step the root still needs, and what running it would change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingUpdateStep {
    pub step: RouterUpdateStep,
    pub change: PlannedChange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlannedChange {
    /// Step 0001 migrates the existing Fleet database to carry the ledger.
    AddLedgerToStore { store: PathBuf },
    /// Step 0001 creates the Fleet database a never-connected Router lacks, with its ledger.
    CreateStoreWithLedger { store: PathBuf },
    /// Step 0005 moves these older stores' rows into the Fleet database, then removes them.
    MergeLegacyStores {
        into: PathBuf,
        sources: Vec<PathBuf>,
    },
    /// Step 0005 with no older store left: it only records itself done.
    NothingToMerge,
    /// Step 0010 renames the Fleet database to its current name.
    RenameFleetDatabase { from: PathBuf, to: PathBuf },
    /// Step 0010 with the file already under its current name: it only records itself done.
    RenameAlreadyDone { store: PathBuf },
}

impl std::fmt::Display for PlannedChange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AddLedgerToStore { store } => {
                write!(formatter, "add the update ledger to {}", store.display())
            }
            Self::CreateStoreWithLedger { store } => {
                write!(
                    formatter,
                    "create {} with the update ledger",
                    store.display()
                )
            }
            Self::MergeLegacyStores { into, sources } => {
                let sources = sources
                    .iter()
                    .map(|source| source.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" and ");
                write!(
                    formatter,
                    "move {sources} into {}, then remove them",
                    into.display()
                )
            }
            Self::NothingToMerge => write!(formatter, "no older Fleet store to move"),
            Self::RenameFleetDatabase { from, to } => {
                write!(formatter, "rename {} to {}", from.display(), to.display())
            }
            Self::RenameAlreadyDone { store } => {
                write!(
                    formatter,
                    "{} already has its current name",
                    store.display()
                )
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RouterUpdateError {
    #[error("the Router Host is running for this root; stop it before running agent-router-update")]
    HostRunning,
    #[error("cannot take this root's Host lock: {0}")]
    HostLock(#[source] InstanceAcquireError),
    #[error(
        "{path} exists, but no update made it; move it aside or remove it, then run agent-router-update again"
    )]
    UnexplainedFleetDatabase { path: PathBuf },
    #[error(
        "both {current} and {former} exist; keep the one that holds this Router's data, move the other aside, then run agent-router-update again"
    )]
    BothFleetDatabaseNames { current: PathBuf, former: PathBuf },
    #[error(
        "{path} still holds data after it was recorded removed; move it aside, then run agent-router-update again"
    )]
    RemovedSourceReappeared { path: PathBuf },
    #[error(
        "{path} still has a write-ahead log or shared-memory file after the checkpoint; the rename stopped so no committed row is lost"
    )]
    LogRemainsAfterCheckpoint { path: PathBuf },
    #[error("{store}: {source}")]
    Store {
        store: PathBuf,
        #[source]
        source: RouterLocalStorageError,
    },
    #[error("{path}: {source}")]
    File {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("update barrier failed: {0}")]
    Barrier(#[source] std::io::Error),
}

/// Where the root's Fleet database is right now, and the ledger it holds.
pub(crate) struct RootLayout {
    pub(crate) store: PathBuf,
    pub(crate) store_is_current: bool,
    pub(crate) ledger: StoredUpdateLedger,
}

impl RootLayout {
    pub(crate) async fn read(root: &Path) -> Result<Self, RouterUpdateError> {
        let current = router_fleet_database(root);
        let former = router_fleet_cache_database(root);
        let (store, store_is_current) = match (exists(&current)?, exists(&former)?) {
            (true, true) => {
                return Err(RouterUpdateError::BothFleetDatabaseNames { current, former });
            }
            (true, false) => (current, true),
            (false, _) => (former, false),
        };
        let ledger =
            read_update_ledger(&store)
                .await
                .map_err(|source| RouterUpdateError::Store {
                    store: store.clone(),
                    source,
                })?;
        if store_is_current && ledger == StoredUpdateLedger::WithoutLedger {
            return Err(RouterUpdateError::UnexplainedFleetDatabase { path: store });
        }
        Ok(Self {
            store,
            store_is_current,
            ledger,
        })
    }

    /// The ledger's progress for one step-0005 source, if it has any.
    pub(crate) fn source_progress(
        &self,
        source: LegacyFleetSource,
    ) -> Option<UpdateSourceProgress> {
        let StoredUpdateLedger::Present(entries) = &self.ledger else {
            return None;
        };
        entries.iter().find_map(|entry| match entry {
            UpdateLedgerEntry::Source {
                step,
                source: name,
                progress,
            } if *step == RouterUpdateStep::MergeFleetStores.number()
                && name == source.file_name() =>
            {
                Some(*progress)
            }
            _ => None,
        })
    }
}

/// A legacy source's database file in this root.
pub(crate) fn legacy_source_path(root: &Path, source: LegacyFleetSource) -> PathBuf {
    router_collaboration_directory(root).join(source.file_name())
}

/// Lists the steps `root` still needs without taking its lock or changing anything.
pub async fn plan_router_update(root: &Path) -> Result<Vec<PendingUpdateStep>, RouterUpdateError> {
    let layout = RootLayout::read(root).await?;
    RouterUpdateStep::KNOWN
        .into_iter()
        .filter(|step| !layout.ledger.step_done(step.number()))
        .map(|step| {
            Ok(PendingUpdateStep {
                change: planned_change(root, step, &layout)?,
                step,
            })
        })
        .collect()
}

/// Runs every pending step under the Host's own singleton lock, so no Host can start
/// on the root while it changes.
pub async fn run_router_update(root: &Path) -> Result<Vec<PendingUpdateStep>, RouterUpdateError> {
    // A root the update refuses is left exactly as found, without even a lock file.
    plan_router_update(root).await?;
    let _host_lock = HostInstanceLock::acquire(&HostCoordinationPaths::for_router_root(root))
        .map_err(|error| match error {
            InstanceAcquireError::AlreadyRunning => RouterUpdateError::HostRunning,
            other => RouterUpdateError::HostLock(other),
        })?;
    // Planned again under the lock: no Host can change the root after this.
    let pending = plan_router_update(root).await?;
    for step in &pending {
        match &step.change {
            PlannedChange::AddLedgerToStore { store } => {
                ledger_step::create_ledger(root, store, LedgerStoreOpen::Existing).await?;
            }
            PlannedChange::CreateStoreWithLedger { store } => {
                ledger_step::create_ledger(root, store, LedgerStoreOpen::Create).await?;
            }
            PlannedChange::MergeLegacyStores { .. } | PlannedChange::NothingToMerge => {
                merge_step::merge_fleet_stores(root).await?;
            }
            PlannedChange::RenameFleetDatabase { .. } | PlannedChange::RenameAlreadyDone { .. } => {
                rename_step::rename_fleet_database(root).await?;
            }
        }
    }
    Ok(pending)
}

fn planned_change(
    root: &Path,
    step: RouterUpdateStep,
    layout: &RootLayout,
) -> Result<PlannedChange, RouterUpdateError> {
    let store = layout.store.clone();
    Ok(match step {
        RouterUpdateStep::CreateLedger => match layout.ledger {
            StoredUpdateLedger::MissingDatabase => PlannedChange::CreateStoreWithLedger { store },
            StoredUpdateLedger::WithoutLedger | StoredUpdateLedger::Present(_) => {
                PlannedChange::AddLedgerToStore { store }
            }
        },
        RouterUpdateStep::MergeFleetStores => {
            let mut sources = Vec::new();
            for source in LegacyFleetSource::ALL {
                let path = legacy_source_path(root, source);
                if exists(&path)? || layout.source_progress(source).is_some() {
                    sources.push(path);
                }
            }
            if sources.is_empty() {
                PlannedChange::NothingToMerge
            } else {
                PlannedChange::MergeLegacyStores {
                    into: store,
                    sources,
                }
            }
        }
        RouterUpdateStep::RenameFleetDatabase => {
            if layout.store_is_current {
                PlannedChange::RenameAlreadyDone { store }
            } else {
                PlannedChange::RenameFleetDatabase {
                    from: store,
                    to: router_fleet_database(root),
                }
            }
        }
    })
}

pub(crate) fn exists(path: &Path) -> Result<bool, RouterUpdateError> {
    path.try_exists().map_err(|source| RouterUpdateError::File {
        path: path.to_owned(),
        source,
    })
}
