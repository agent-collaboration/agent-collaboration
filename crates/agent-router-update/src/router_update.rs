//! Runs the one-time steps a Router root still needs, in order, with its Host stopped.
//!
//! The step catalog (`RouterUpdateStep`) is the one the Host's start-up guard reads,
//! so the steps run here are exactly the ones it refuses on until they are done.
use crate::{
    ledger_step::{self, LedgerStoreOpen},
    merge_step, rename_step,
    root_layout::{RootLayout, RouterUpdateError, exists, legacy_source_path},
};
use codex_router_host::{
    HostCoordinationPaths, HostInstanceLock, InstanceAcquireError, router_fleet_database,
};
use message_board_storage::{
    LegacyFleetSource, RouterUpdateStep, StoredUpdateLedger, UpdateSourceProgress,
};
use std::path::{Path, PathBuf};

/// What an update would do to a root: its pending steps, in order, and the older stores
/// that reappeared after step 0005 had already moved them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RouterUpdatePlan {
    pub steps: Vec<PendingUpdateStep>,
    /// Database files, with any log or shared-memory file beside them, removed unread:
    /// their rows were carried and checked once already, so these copies hold nothing
    /// the Fleet database lacks and are never merged a second time.
    pub stale_sources: Vec<PathBuf>,
}

impl RouterUpdatePlan {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.stale_sources.is_empty()
    }
}

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

/// Lists what `root` still needs without taking its lock or changing anything.
pub async fn plan_router_update(root: &Path) -> Result<RouterUpdatePlan, RouterUpdateError> {
    let layout = RootLayout::read(root).await?;
    let steps = RouterUpdateStep::KNOWN
        .into_iter()
        .filter(|step| !layout.ledger.step_done(step.number()))
        .map(|step| {
            Ok(PendingUpdateStep {
                change: planned_change(root, step, &layout)?,
                step,
            })
        })
        .collect::<Result<_, RouterUpdateError>>()?;
    Ok(RouterUpdatePlan {
        steps,
        stale_sources: stale_sources(root, &layout)?,
    })
}

/// An older store is stale once step 0005 is done, or once the ledger records that
/// store itself removed: either way its rows already moved and were checked.
fn stale_sources(root: &Path, layout: &RootLayout) -> Result<Vec<PathBuf>, RouterUpdateError> {
    let merge_done = layout
        .ledger
        .step_done(RouterUpdateStep::MergeFleetStores.number());
    let mut stale = Vec::new();
    for source in LegacyFleetSource::ALL {
        let removed = layout.source_progress(source) == Some(UpdateSourceProgress::Removed);
        let path = legacy_source_path(root, source);
        if (merge_done || removed) && merge_step::any_store_file_present(&path)? {
            stale.push(path);
        }
    }
    Ok(stale)
}

/// Runs every pending step under the Host's own singleton lock, so no Host can start
/// on the root while it changes.
pub async fn run_router_update(root: &Path) -> Result<RouterUpdatePlan, RouterUpdateError> {
    // A root the update refuses is left exactly as found, without even a lock file.
    plan_router_update(root).await?;
    let _host_lock = HostInstanceLock::acquire(&HostCoordinationPaths::for_router_root(root))
        .map_err(|error| match error {
            InstanceAcquireError::AlreadyRunning => RouterUpdateError::HostRunning,
            other => RouterUpdateError::HostLock(other),
        })?;
    // Planned again under the lock: no Host can change the root after this.
    let plan = plan_router_update(root).await?;
    for stale in &plan.stale_sources {
        merge_step::remove_stale_source(stale).await?;
    }
    for step in &plan.steps {
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
    Ok(plan)
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
