//! State one run of a session owns (design note §11.3): the settings
//! catalog, the capability report and the in-folder grant. (The fast hold
//! is the session's, not a run's: it lives in the session cell, §12.3.)
//! Each entry carries the generation that wrote it. A write from a
//! run that isn't the session's current one, or older than the stored
//! entry, changes nothing; a read answers only for the session's current
//! run. So a task of an ended run (a held settings continuation, a late
//! prompt observation) can never overwrite or be read as a later run's state.
//! Every access checks the run and touches the map under the session's cell
//! lock, always cell then map (review F5): a run's end, which moves the cell
//! first, can't fall between a writer's check and its write.

use crate::session_cell_table::SessionCellTable;
use provider_session_machines::SessionGeneration;
use provider_session_machines::session_lifecycle_machine::SessionLifecycle;
use provider_session_machines::session_run_claims::{RunClaim, run_admits};
use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

/// What a write did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunWrite {
    Written,
    /// The writer's run isn't the session's current one, or a later run has
    /// written: nothing changed.
    StaleRun,
}

struct RunOwned<T> {
    generation: SessionGeneration,
    value: T,
}

pub(crate) struct RunOwnedMap<T> {
    cells: Arc<SessionCellTable>,
    entries: RwLock<HashMap<String, RunOwned<T>>>,
}

impl<T: Clone> RunOwnedMap<T> {
    pub(crate) fn new(cells: Arc<SessionCellTable>) -> Self {
        Self {
            cells,
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Replaces the session's entry for `generation`.
    pub(crate) fn write(
        &self,
        session_id: &str,
        generation: SessionGeneration,
        value: T,
    ) -> RunWrite {
        self.write_with(session_id, generation, |entry| *entry = Some(value))
    }

    /// Removes the session's entry for `generation`: a restore with nothing
    /// to grant still decides for its run.
    pub(crate) fn clear(&self, session_id: &str, generation: SessionGeneration) -> RunWrite {
        self.write_with(session_id, generation, |entry| *entry = None)
    }

    /// Changes the session's entry for `generation` in place; a missing or
    /// earlier run's entry reads as absent to `change`.
    pub(crate) fn write_with(
        &self,
        session_id: &str,
        generation: SessionGeneration,
        change: impl FnOnce(&mut Option<T>),
    ) -> RunWrite {
        self.cells.read(session_id, |cell| {
            if !run_admits(&cell.lifecycle, RunClaim::Exact(generation)) {
                return RunWrite::StaleRun;
            }
            let mut entries = self.entries.write().unwrap_or_else(PoisonError::into_inner);
            let mut entry = match entries.remove(session_id) {
                Some(stored) if stored.generation > generation => {
                    entries.insert(session_id.to_owned(), stored);
                    return RunWrite::StaleRun;
                }
                Some(stored) if stored.generation == generation => Some(stored.value),
                Some(_) | None => None,
            };
            change(&mut entry);
            if let Some(value) = entry {
                entries.insert(session_id.to_owned(), RunOwned { generation, value });
            }
            RunWrite::Written
        })
    }

    /// The session's entry, only if its current run wrote it.
    pub(crate) fn read(&self, session_id: &str) -> Option<T> {
        self.cells.read(session_id, |cell| {
            let current = live_generation(&cell.lifecycle)?;
            self.entries
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .get(session_id)
                .filter(|stored| stored.generation == current)
                .map(|stored| stored.value.clone())
        })
    }

    /// The session's entry for a task of run `generation`: only that run's
    /// own entry, and only while it is the session's current run.
    pub(crate) fn read_in(&self, session_id: &str, generation: SessionGeneration) -> Option<T> {
        self.cells.read(session_id, |cell| {
            if !run_admits(&cell.lifecycle, RunClaim::Exact(generation)) {
                return None;
            }
            self.entries
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .get(session_id)
                .filter(|stored| stored.generation == generation)
                .map(|stored| stored.value.clone())
        })
    }

    /// The run `generation` ended: its entry goes, a later run's stays.
    /// Under the cell lock, cell then map, like every other access.
    pub(crate) fn end_run(&self, session_id: &str, generation: SessionGeneration) {
        self.cells.read(session_id, |_cell| {
            let mut entries = self.entries.write().unwrap_or_else(PoisonError::into_inner);
            if entries
                .get(session_id)
                .is_some_and(|stored| stored.generation <= generation)
            {
                entries.remove(session_id);
            }
        });
    }

    /// Connection-wide: applies `change` to every session's entry, whatever
    /// its run (the agent's auth status belongs to the connection).
    pub(crate) fn update_every(&self, mut change: impl FnMut(&str, &mut T)) {
        for (session_id, stored) in self
            .entries
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .iter_mut()
        {
            change(session_id, &mut stored.value);
        }
    }
}

/// A run ended (closed, or its actor exited): every piece of its own state
/// goes; a later run's stays.
pub(crate) fn end_run_state(
    session_id: &str,
    generation: SessionGeneration,
    capabilities: &RunOwnedMap<crate::ProviderCapabilityReport>,
    settings: &RunOwnedMap<crate::ProviderSettingsCatalog>,
    grants: &RunOwnedMap<crate::in_folder_allowance::SessionInFolderGrant>,
) {
    capabilities.end_run(session_id, generation);
    settings.end_run(session_id, generation);
    grants.end_run(session_id, generation);
}

/// The run a session has now, if it has one.
fn live_generation(lifecycle: &SessionLifecycle) -> Option<SessionGeneration> {
    match lifecycle {
        SessionLifecycle::Loading { generation, .. }
        | SessionLifecycle::Live { generation }
        | SessionLifecycle::Closing { generation, .. } => Some(*generation),
        SessionLifecycle::NotLoaded { .. } => None,
    }
}

#[cfg(test)]
#[path = "run_owned_state_test.rs"]
mod tests;
