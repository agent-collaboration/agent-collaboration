//! One provider connection's session cells (D1 state machines, design note
//! §11.2, §11.7): each provider session's `SessionCell` behind its own lock.
//! The table's index lock covers only lookup and insert; a transition holds
//! one session's lock for the pure move and never across an await, so
//! sessions never wait on each other.
//!
//! Each transition's message effects go to exactly one place: back to the
//! caller when the Host drove the move (it runs them through its dispatch
//! path), or to the Host's queue sink when another move won (a close, a
//! failed load, an actor exit, a stop) and the Host must settle the messages
//! it ended.

use provider_session_machines::queued_entry_machine::QueuedEntryEffect;
use provider_session_machines::session_cell_machine::{
    SessionCell, SessionCellEffect, SessionCellEvent, SessionCellMachine, SessionCellRefusal,
};
use state_machine_core::{StateMachine, TransitionError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

/// Settles queued messages that a move the Host didn't drive ended: a close,
/// a failed load, an actor exit or a stop. Called after the session's lock is
/// released; it must not block.
pub trait QueueEntrySink: Send + Sync {
    /// The message `operation_id` in `session_id`'s line ends not sent.
    fn entry_not_sent(&self, session_id: &str, operation_id: &str, reason: &'static str);
}

/// Where a transition's message effects go.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryEffectRoute {
    /// The Host drove the move and runs its message effects.
    Caller,
    /// Another move won; the Host's sink settles the messages it ended.
    Sink,
}

#[derive(Default)]
pub(crate) struct SessionCellTable {
    cells: Mutex<HashMap<String, Arc<Mutex<SessionCell>>>>,
    sink: OnceLock<Arc<dyn QueueEntrySink>>,
    /// Wakes callers waiting for a session's cell to leave a state.
    changed: tokio::sync::Notify,
}

impl SessionCellTable {
    /// Sets the Host's sink once; a second call keeps the first.
    pub(crate) fn set_sink(&self, sink: Arc<dyn QueueEntrySink>) {
        let _kept = self.sink.set(sink);
    }

    /// Applies `event` to the session's cell. A session this connection
    /// hasn't run starts NotLoaded; only a create or a load can move it. Nothing changes unless the move
    /// succeeds, so a refused event never adds a cell. With the Sink route,
    /// message effects go to the sink and the rest come back.
    pub(crate) fn transition(
        &self,
        session_id: &str,
        event: SessionCellEvent,
        route: EntryEffectRoute,
    ) -> Result<Vec<SessionCellEffect>, TransitionError<SessionCellRefusal>> {
        let effects = self.apply(session_id, event)?;
        self.changed.notify_waiters();
        Ok(match route {
            EntryEffectRoute::Caller => effects,
            EntryEffectRoute::Sink => self.deliver_entry_effects(session_id, effects),
        })
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

    /// Waits while `holds` is true of the session's cell, rechecking after
    /// every successful move on the table.
    pub(crate) async fn wait_while(&self, session_id: &str, holds: impl Fn(&SessionCell) -> bool) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.read(session_id, &holds) {
                return;
            }
            changed.await;
        }
    }

    /// Every session this connection has a cell for.
    pub(crate) fn session_ids(&self) -> Vec<String> {
        lock(&self.cells).keys().cloned().collect()
    }

    fn apply(
        &self,
        session_id: &str,
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
            let seeded = SessionCell::fresh();
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

    /// Hands message settlements to the sink and returns the other effects.
    fn deliver_entry_effects(
        &self,
        session_id: &str,
        effects: Vec<SessionCellEffect>,
    ) -> Vec<SessionCellEffect> {
        let mut rest = Vec::new();
        for effect in effects {
            let SessionCellEffect::Entry {
                operation, effect, ..
            } = &effect
            else {
                rest.push(effect);
                continue;
            };
            let reason = match effect {
                QueuedEntryEffect::SettleNotSent(reason)
                | QueuedEntryEffect::ReadBackRow(reason)
                | QueuedEntryEffect::OverlayNotSentOverDispatch(reason) => reason.human_phrase(),
                // A sent message settles through its own turn; the moves that
                // start, mark or unmark a send are the Host's own.
                QueuedEntryEffect::SettleSent(_)
                | QueuedEntryEffect::StartSend
                | QueuedEntryEffect::WriteMark
                | QueuedEntryEffect::SendRequest
                | QueuedEntryEffect::Unmark => continue,
            };
            match self.sink.get() {
                Some(sink) => sink.entry_not_sent(session_id, &operation.0, reason),
                None => tracing::warn!(
                    session_id,
                    operation = operation.0.as_str(),
                    reason,
                    "a queued message ended with no queue sink to settle it"
                ),
            }
        }
        rest
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The sink reads the connection's runs from its cells (D1 step 6): a
/// claim is checked, and the publication enqueued, under the session cell's
/// lock.
impl crate::SessionRuns for SessionCellTable {
    fn publish_if(
        &self,
        session_id: &str,
        claim: provider_session_machines::session_run_claims::RunClaim,
        enqueue: &mut dyn FnMut() -> Result<(), crate::session_event_sink::EventSinkClosed>,
    ) -> Result<
        crate::session_event_sink::GenerationPublication,
        crate::session_event_sink::EventSinkClosed,
    > {
        self.read(session_id, |cell| {
            if provider_session_machines::session_run_claims::run_admits(&cell.lifecycle, claim) {
                enqueue().map(|()| crate::session_event_sink::GenerationPublication::Published)
            } else {
                Ok(crate::session_event_sink::GenerationPublication::Ended)
            }
        })
    }

    fn current_generation(&self, session_id: &str) -> provider_session_machines::SessionGeneration {
        self.read(session_id, |cell| match cell.lifecycle {
            provider_session_machines::session_lifecycle_machine::SessionLifecycle::NotLoaded {
                last,
            } => last,
            provider_session_machines::session_lifecycle_machine::SessionLifecycle::Loading {
                generation,
                ..
            }
            | provider_session_machines::session_lifecycle_machine::SessionLifecycle::Live {
                generation,
            }
            | provider_session_machines::session_lifecycle_machine::SessionLifecycle::Closing {
                generation,
                ..
            } => generation,
        })
    }
}

#[cfg(test)]
#[path = "session_cell_table_test.rs"]
mod tests;
