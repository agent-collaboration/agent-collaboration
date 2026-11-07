//! Whether a Router root must run `agent-router-update` before the Host or an account
//! command may use it. The step catalog lives with the ledger (`RouterUpdateStep`): the
//! update tool runs exactly those steps, and a store the Host creates records them all
//! done, so an empty or partial ledger always means an interrupted update.
use message_board_storage::{
    FleetStore, LegacyFleetSource, RouterLocalStorageError, RouterUpdateStep, StoredUpdateLedger,
    UpdateLedgerEntry, UpdateStepNumber, UpdateStepProgress, read_update_ledger,
};
use std::path::{Path, PathBuf};

/// The Fleet database under its name until the update renames it.
#[must_use]
pub fn router_fleet_cache_database(root: &Path) -> PathBuf {
    crate::router_collaboration_directory(root).join("fleet-cache.sqlite")
}

/// The Fleet database under the name the update gives it.
#[must_use]
pub fn router_fleet_database(root: &Path) -> PathBuf {
    crate::router_collaboration_directory(root).join(FleetStore::FILE_NAME)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouterUpdateRequirement {
    NotRequired,
    Required(RequiredRouterUpdate),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequiredRouterUpdate {
    /// An older Router made this store before the ledger existed.
    StoreWithoutLedger { store: PathBuf },
    /// The update started on this root, and these steps aren't done.
    UnfinishedSteps {
        store: PathBuf,
        steps: Vec<UpdateStepNumber>,
    },
    /// The Fleet database exists under both its former and its current name.
    BothFleetDatabaseNames { current: PathBuf, former: PathBuf },
    /// A store the update merges or renames away is still here after its step.
    LegacyStoreRemains { path: PathBuf },
}

impl std::fmt::Display for RequiredRouterUpdate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StoreWithoutLedger { store } => write!(
                formatter,
                "{} was made by an older Router; run agent-router-update with the Host stopped",
                store.display()
            ),
            Self::UnfinishedSteps { store, steps } => {
                let steps = steps
                    .iter()
                    .map(|step| format!("{:04}", step.get()))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    formatter,
                    "the update of this Router root is unfinished (steps {steps} in {}); run agent-router-update with the Host stopped",
                    store.display()
                )
            }
            Self::BothFleetDatabaseNames { current, former } => write!(
                formatter,
                "both {} and {} exist; run agent-router-update with the Host stopped, which names the conflict",
                current.display(),
                former.display()
            ),
            Self::LegacyStoreRemains { path } => write!(
                formatter,
                "{} is left from an older layout; run agent-router-update with the Host stopped",
                path.display()
            ),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("cannot read the update ledger in {store}: {source}")]
pub struct RouterUpdateInspectionError {
    store: PathBuf,
    #[source]
    source: RouterLocalStorageError,
}

/// Decides whether `root` needs the update, reading its ledger without the store's
/// lock and without creating or migrating anything.
pub async fn inspect_router_update_requirement(
    root: &Path,
) -> Result<RouterUpdateRequirement, RouterUpdateInspectionError> {
    let current = router_fleet_database(root);
    let former = router_fleet_cache_database(root);
    // An unreadable existence check counts as present, so the guard fails closed.
    let exists = |path: &Path| path.try_exists().unwrap_or(true);
    let (current_exists, former_exists) = (exists(&current), exists(&former));
    if current_exists && former_exists {
        return Ok(RouterUpdateRequirement::Required(
            RequiredRouterUpdate::BothFleetDatabaseNames { current, former },
        ));
    }
    let store = if current_exists {
        current
    } else {
        former.clone()
    };
    let ledger =
        read_update_ledger(&store)
            .await
            .map_err(|source| RouterUpdateInspectionError {
                store: store.clone(),
                source,
            })?;
    let requirement = requirement_for_ledger(store, &ledger);
    if requirement != RouterUpdateRequirement::NotRequired {
        return Ok(requirement);
    }
    // Every step is done, so nothing from the older layout may be left: not the Fleet
    // database under its former name, and not a store step 0005 merged away.
    let directory = crate::router_collaboration_directory(root);
    let leftover = former_exists.then_some(former).or_else(|| {
        LegacyFleetSource::ALL
            .iter()
            .map(|source| directory.join(source.file_name()))
            .find(|path| exists(path))
    });
    Ok(match leftover {
        Some(path) => {
            RouterUpdateRequirement::Required(RequiredRouterUpdate::LegacyStoreRemains { path })
        }
        None => RouterUpdateRequirement::NotRequired,
    })
}

fn requirement_for_ledger(store: PathBuf, ledger: &StoredUpdateLedger) -> RouterUpdateRequirement {
    let entries = match ledger {
        // A root whose Host never opened the Fleet database has nothing to update.
        StoredUpdateLedger::MissingDatabase => return RouterUpdateRequirement::NotRequired,
        StoredUpdateLedger::WithoutLedger => {
            return RouterUpdateRequirement::Required(RequiredRouterUpdate::StoreWithoutLedger {
                store,
            });
        }
        StoredUpdateLedger::Present(entries) => entries,
    };
    let mut unfinished: Vec<UpdateStepNumber> = RouterUpdateStep::KNOWN
        .iter()
        .map(|step| step.number())
        .filter(|number| !ledger.step_done(*number))
        .collect();
    // A step left in progress counts even if this Router doesn't know it, such as
    // one a newer update tool was running when it stopped.
    for entry in entries {
        if let UpdateLedgerEntry::Step {
            step,
            progress: UpdateStepProgress::InProgress,
        } = entry
            && !unfinished.contains(step)
        {
            unfinished.push(*step);
        }
    }
    if unfinished.is_empty() {
        RouterUpdateRequirement::NotRequired
    } else {
        unfinished.sort();
        RouterUpdateRequirement::Required(RequiredRouterUpdate::UnfinishedSteps {
            store,
            steps: unfinished,
        })
    }
}

#[cfg(test)]
#[path = "router_update_requirement_test.rs"]
mod tests;
