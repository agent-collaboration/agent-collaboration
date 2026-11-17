//! One provider connection's session cells (D1 state machines, design note
//! §11.2, §11.7): each provider session's `SessionCell` behind its own lock.
//! The table's index lock covers only lookup and insert; a transition holds
//! one session's lock for the pure move and never across an await, so
//! sessions never wait on each other. The caller runs the returned effects.

use crate::SessionGeneration;
use crate::session_cell_machine::{
    SessionCell, SessionCellEffect, SessionCellEvent, SessionCellMachine, SessionCellRefusal,
};
use state_machine_core::{StateMachine, TransitionError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[derive(Default)]
pub(crate) struct SessionCellTable {
    cells: Mutex<HashMap<String, Arc<Mutex<SessionCell>>>>,
}

impl SessionCellTable {
    /// Applies `event` to the session's cell. A session this connection
    /// hasn't run starts NotLoaded after `last_published`, the generation
    /// its last run on an earlier connection published under; only a create
    /// or a load can move such a cell. Nothing changes unless the move
    /// succeeds, so a refused event never adds a cell.
    pub(crate) fn transition(
        &self,
        session_id: &str,
        last_published: SessionGeneration,
        event: SessionCellEvent,
    ) -> Result<Vec<SessionCellEffect>, TransitionError<SessionCellRefusal>> {
        loop {
            let existing = lock(&self.cells).get(session_id).map(Arc::clone);
            if let Some(cell) = existing {
                let mut cell = lock(&cell);
                let (next, effects) = SessionCellMachine::transition(&cell, event)?;
                *cell = next;
                return Ok(effects);
            }
            let seeded = SessionCell::not_loaded_after(last_published);
            let (next, effects) = SessionCellMachine::transition(&seeded, event.clone())?;
            let mut cells = lock(&self.cells);
            if cells.contains_key(session_id) {
                // Another move added the cell first: apply to that one.
                continue;
            }
            cells.insert(session_id.to_owned(), Arc::new(Mutex::new(next)));
            return Ok(effects);
        }
    }

    /// Reads the session's cell; a session this connection hasn't run reads
    /// as a fresh cell.
    pub(crate) fn read<R>(&self, session_id: &str, read: impl FnOnce(&SessionCell) -> R) -> R {
        let cell = lock(&self.cells).get(session_id).map(Arc::clone);
        match cell {
            Some(cell) => read(&lock(&cell)),
            None => read(&SessionCell::fresh()),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
