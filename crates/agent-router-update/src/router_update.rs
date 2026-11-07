//! Runs the one-time steps a Router root still needs, in order, with its Host stopped.
//!
//! The step catalog (`RouterUpdateStep`) is the one the Host's start-up guard reads,
//! so the steps run here are exactly the ones it refuses on until they are done.
use crate::{
    dead_files_step::{self, DeadFileRemoval},
    ledger_step::{self, LedgerStoreOpen},
    merge_step, rename_step, reset_step,
    root_layout::{RootLayout, RouterUpdateError, exists, legacy_source_path},
};
use codex_router_host::{
    HostCoordinationPaths, HostInstanceLock, InstanceAcquireError,
    fleet_enrollment_restart_required, router_dead_files, router_fleet_database,
};
use message_board_storage::{
    LegacyFleetSource, RouterUpdateStep, StoredUpdateLedger, UpdateSourceProgress,
};
use std::path::{Path, PathBuf};

/// What an update would do to a root: its pending steps, in order, and the files that
/// reappeared after the step that handled them was done.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RouterUpdatePlan {
    pub steps: Vec<PendingUpdateStep>,
    /// Removed unread: a step that is done never reads what it already handled again.
    pub stale: Vec<StaleFile>,
}

impl RouterUpdatePlan {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.stale.is_empty()
    }
}

/// A file found after the step that handled it was done.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StaleFile {
    /// An older store step 0005 already carried and checked, with any log or
    /// shared-memory file beside it. These copies hold nothing the Fleet database lacks.
    MergedStore(PathBuf),
    /// A file step 0009 already removed, which nothing reads.
    DeadFile(PathBuf),
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
        }
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
    /// Step 0002 finishes the enrollment reset recorded in this journal against the store.
    FinishEnrollmentReset { journal: PathBuf, store: PathBuf },
    /// Step 0002 with no unfinished reset: it only records itself done.
    NoEnrollmentResetToFinish,
    /// Step 0005 moves these older stores' rows into the Fleet database, then removes them.
    MergeLegacyStores {
        into: PathBuf,
        sources: Vec<PathBuf>,
    },
    /// Step 0005 with no older store left: it only records itself done.
    NothingToMerge,
    /// Step 0009 imports these approval routes, if any, then removes these paths.
    RemoveDeadFiles {
        import_routes: Option<PathBuf>,
        remove: Vec<PathBuf>,
    },
    /// Step 0009 with nothing to remove: it only records itself done.
    NoDeadFiles,
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
            Self::FinishEnrollmentReset { journal, store } => write!(
                formatter,
                "finish the enrollment reset recorded in {} against {}",
                journal.display(),
                store.display()
            ),
            Self::NoEnrollmentResetToFinish => {
                write!(formatter, "no unfinished enrollment reset")
            }
            Self::RemoveDeadFiles {
                import_routes,
                remove,
            } => {
                if let Some(routes) = import_routes {
                    write!(
                        formatter,
                        "import the approval routes in {} into interaction.sqlite, then ",
                        routes.display()
                    )?;
                }
                let remove = remove
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(formatter, "remove {remove}")
            }
            Self::NoDeadFiles => write!(formatter, "no dead file to remove"),
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
        stale: stale_files(root, &layout)?,
    })
}

/// An older store is stale once step 0005 is done, or once the ledger records that
/// store itself removed: either way its rows already moved and were checked. A dead
/// file is stale once step 0009 is done.
fn stale_files(root: &Path, layout: &RootLayout) -> Result<Vec<StaleFile>, RouterUpdateError> {
    let merge_done = layout
        .ledger
        .step_done(RouterUpdateStep::MergeFleetStores.number());
    let mut stale = Vec::new();
    for source in LegacyFleetSource::ALL {
        let removed = layout.source_progress(source) == Some(UpdateSourceProgress::Removed);
        let path = legacy_source_path(root, source);
        if (merge_done || removed) && merge_step::any_store_file_present(&path)? {
            stale.push(StaleFile::MergedStore(path));
        }
    }
    if layout
        .ledger
        .step_done(RouterUpdateStep::RemoveDeadFiles.number())
    {
        let dead = router_dead_files(root).map_err(|source| RouterUpdateError::File {
            path: codex_router_host::router_collaboration_directory(root),
            source,
        })?;
        stale.extend(dead.into_iter().map(StaleFile::DeadFile));
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
    for stale in &plan.stale {
        match stale {
            StaleFile::MergedStore(path) => merge_step::remove_stale_source(path).await?,
            StaleFile::DeadFile(path) => merge_step::remove_if_present(path).await?,
        }
    }
    for step in &plan.steps {
        match &step.change {
            PlannedChange::AddLedgerToStore { store } => {
                ledger_step::create_ledger(root, store, LedgerStoreOpen::Existing).await?;
            }
            PlannedChange::CreateStoreWithLedger { store } => {
                ledger_step::create_ledger(root, store, LedgerStoreOpen::Create).await?;
            }
            PlannedChange::FinishEnrollmentReset { .. }
            | PlannedChange::NoEnrollmentResetToFinish => {
                reset_step::finish_enrollment_reset(root).await?;
            }
            PlannedChange::RemoveDeadFiles { .. } | PlannedChange::NoDeadFiles => {
                dead_files_step::remove_dead_files(root).await?;
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
        RouterUpdateStep::FinishEnrollmentReset => {
            let journal = reset_step::enrollment_reset_journal(root);
            let unfinished = fleet_enrollment_restart_required(root).map_err(|source| {
                RouterUpdateError::File {
                    path: journal.clone(),
                    source,
                }
            })?;
            if unfinished {
                PlannedChange::FinishEnrollmentReset { journal, store }
            } else {
                PlannedChange::NoEnrollmentResetToFinish
            }
        }
        RouterUpdateStep::RemoveDeadFiles => {
            let reset_still_to_finish = !layout
                .ledger
                .step_done(RouterUpdateStep::FinishEnrollmentReset.number());
            let DeadFileRemoval {
                import_routes,
                remove,
            } = dead_files_step::dead_file_removal(root, reset_still_to_finish)?;
            if remove.is_empty() {
                PlannedChange::NoDeadFiles
            } else {
                PlannedChange::RemoveDeadFiles {
                    import_routes,
                    remove,
                }
            }
        }
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
