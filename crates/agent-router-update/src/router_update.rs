//! Runs the one-time steps a Router root still needs, in order, with its Host stopped.
//!
//! The step catalog (`RouterUpdateStep`) is the one the Host's start-up guard reads,
//! so the steps run here are exactly the ones it refuses on until they are done.
use crate::update_barrier::{UpdateBarrier, wait_at};
use codex_router_host::{
    HostCoordinationPaths, HostInstanceLock, InstanceAcquireError, router_fleet_cache_database,
    router_fleet_database,
};
use message_board_storage::{
    RouterLocalStorageError, RouterUpdateStep, StoredUpdateLedger, UpdateLedger,
    UpdateStepProgress, read_update_ledger,
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

/// Lists the steps `root` still needs without taking its lock or changing anything.
pub async fn plan_router_update(root: &Path) -> Result<Vec<PendingUpdateStep>, RouterUpdateError> {
    refuse_unexplained_fleet_database(root).await?;
    let store = router_fleet_cache_database(root);
    let ledger = read_update_ledger(&store)
        .await
        .map_err(|source| RouterUpdateError::Store {
            store: store.clone(),
            source,
        })?;
    Ok(RouterUpdateStep::KNOWN
        .into_iter()
        .filter(|step| !ledger.step_done(step.number()))
        .map(|step| PendingUpdateStep {
            change: planned_change(step, &store, &ledger),
            step,
        })
        .collect())
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
        run_step(root, step).await?;
    }
    Ok(pending)
}

fn planned_change(
    step: RouterUpdateStep,
    store: &Path,
    ledger: &StoredUpdateLedger,
) -> PlannedChange {
    match step {
        RouterUpdateStep::CreateLedger => match ledger {
            StoredUpdateLedger::MissingDatabase => PlannedChange::CreateStoreWithLedger {
                store: store.to_owned(),
            },
            StoredUpdateLedger::WithoutLedger | StoredUpdateLedger::Present(_) => {
                PlannedChange::AddLedgerToStore {
                    store: store.to_owned(),
                }
            }
        },
    }
}

async fn run_step(root: &Path, pending: &PendingUpdateStep) -> Result<(), RouterUpdateError> {
    match pending.step {
        RouterUpdateStep::CreateLedger => create_ledger(root, &pending.change).await,
    }
}

/// Step 0001. Every later step reads and writes this ledger, which stays in the file
/// it describes for the rest of the update.
async fn create_ledger(root: &Path, change: &PlannedChange) -> Result<(), RouterUpdateError> {
    let store = router_fleet_cache_database(root);
    let storage_error = |source| RouterUpdateError::Store {
        store: store.clone(),
        source,
    };
    let ledger = match change {
        PlannedChange::AddLedgerToStore { .. } => UpdateLedger::migrate_existing(&store)
            .await
            .map_err(storage_error)?,
        PlannedChange::CreateStoreWithLedger { .. } => {
            let directory = codex_router_host::router_collaboration_directory(root);
            tokio::fs::create_dir_all(&directory)
                .await
                .map_err(|source| RouterUpdateError::File {
                    path: directory,
                    source,
                })?;
            UpdateLedger::create(&store).await.map_err(storage_error)?
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

/// No step in this release makes `fleet.sqlite`, so one already there came from
/// somewhere else, and the update won't build on it.
async fn refuse_unexplained_fleet_database(root: &Path) -> Result<(), RouterUpdateError> {
    let path = router_fleet_database(root);
    match tokio::fs::symlink_metadata(&path).await {
        Ok(_) => Err(RouterUpdateError::UnexplainedFleetDatabase { path }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(RouterUpdateError::File { path, source }),
    }
}
