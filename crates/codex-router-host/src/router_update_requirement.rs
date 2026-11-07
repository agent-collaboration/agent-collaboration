//! The one-time update steps this Router knows, and whether a Router root must run
//! `agent-router-update` before the Host or an account command may use it.
//!
//! The update tool runs exactly these steps, and the guard refuses on exactly
//! their markers, so a step the tool can't run never makes the Host refuse.
use message_board_storage::{
    RouterLocalStorageError, StoredUpdateLedger, UpdateLedgerEntry, UpdateStepNumber,
    UpdateStepProgress, read_update_ledger,
};
use std::path::{Path, PathBuf};

/// One one-time step, in the order the update runs them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouterUpdateStep {
    /// 0001: the Fleet database gains the update ledger before anything moves.
    CreateLedger,
}

impl RouterUpdateStep {
    pub const KNOWN: [Self; 1] = [Self::CreateLedger];

    #[must_use]
    pub const fn number(self) -> UpdateStepNumber {
        match self {
            Self::CreateLedger => UpdateStepNumber::new(1),
        }
    }

    /// The step's ledger number as the owner sees it, such as "0001".
    #[must_use]
    pub fn label(self) -> String {
        format!("{:04}", self.number().get())
    }
}

/// The Fleet database under its name until the update renames it.
#[must_use]
pub fn router_fleet_cache_database(root: &Path) -> PathBuf {
    crate::router_collaboration_directory(root).join("fleet-cache.sqlite")
}

/// The Fleet database under the name the update gives it.
#[must_use]
pub fn router_fleet_database(root: &Path) -> PathBuf {
    crate::router_collaboration_directory(root).join("fleet.sqlite")
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
    let store = router_fleet_cache_database(root);
    let ledger =
        read_update_ledger(&store)
            .await
            .map_err(|source| RouterUpdateInspectionError {
                store: store.clone(),
                source,
            })?;
    Ok(requirement_for_ledger(store, &ledger))
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
    // An empty ledger is a store this Router's own Host made: the update never ran,
    // and nothing old is left for it to do.
    if entries.is_empty() {
        return RouterUpdateRequirement::NotRequired;
    }
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
