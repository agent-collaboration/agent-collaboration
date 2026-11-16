//! A session line's submission gate (D1 R14). Stopping the line, closing its
//! session and committing a claimed entry's provider submission all take the
//! same lock, so each happens wholly before or wholly after the others. A stop
//! or close that wins ends the entry not sent; a submission that wins keeps its
//! real outcome.

use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub(crate) struct QueueSubmissionGate {
    state: Arc<Mutex<GateState>>,
}

#[derive(Default)]
struct GateState {
    /// Why the whole line stopped (Router shutdown), once it did.
    stopped: Option<String>,
    /// Entries numbered below this were in the line when its session closed.
    closed_before: u64,
    /// Why the session closed, when it did.
    close_reason: Option<String>,
    /// The Router closed the session itself (an agent with no session/close):
    /// every entry, whenever it entered the line, ends not sent (D1 R14).
    terminal: Option<String>,
}

impl QueueSubmissionGate {
    /// Stops the line: no entry commits a submission after this. The first
    /// reason stays.
    pub(crate) fn stop(&self, reason: &str) {
        let mut state = self.lock();
        if state.stopped.is_none() {
            state.stopped = Some(reason.to_owned());
        }
    }

    /// Closes the session for every entry numbered below `sent`.
    pub(crate) fn close_before(&self, sent: u64, reason: &str) {
        let mut state = self.lock();
        state.close_reason = Some(reason.to_owned());
        state.closed_before = sent;
    }

    /// Closes the session for good: no entry commits a submission after this,
    /// whenever it entered the line.
    pub(crate) fn close_terminally(&self, reason: &str) {
        let mut state = self.lock();
        if state.terminal.is_none() {
            state.terminal = Some(reason.to_owned());
        }
    }

    /// The close reason, when entry `sequence` was in the line as its session
    /// closed, or for any entry once the session closed for good.
    pub(crate) fn closed_reason_for(&self, sequence: u64) -> Option<String> {
        let state = self.lock();
        if let Some(reason) = &state.terminal {
            return Some(reason.clone());
        }
        (sequence < state.closed_before)
            .then(|| state.close_reason.clone())
            .flatten()
    }

    /// Runs `submit` for entry `sequence` unless the line stopped, its session
    /// closed over it, or `retirement` fired; otherwise returns that reason
    /// and runs nothing. `submit` runs under the gate's lock, so it must only
    /// start the submission (spawn it), never wait on it.
    pub(crate) fn commit<T>(
        &self,
        sequence: u64,
        retirement: &CancellationToken,
        retirement_reason: &str,
        submit: impl FnOnce() -> T,
    ) -> Result<T, String> {
        let state = self.lock();
        if let Some(reason) = &state.stopped {
            return Err(reason.clone());
        }
        if let Some(reason) = &state.terminal {
            return Err(reason.clone());
        }
        if sequence < state.closed_before
            && let Some(reason) = &state.close_reason
        {
            return Err(reason.clone());
        }
        if retirement.is_cancelled() {
            return Err(retirement_reason.to_owned());
        }
        Ok(submit())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// What a claimed queued dispatch's owner needs to commit its submission
/// through its line's gate.
#[derive(Clone)]
pub(crate) struct QueuedSubmissionCommit {
    gate: QueueSubmissionGate,
    sequence: u64,
    retirement: CancellationToken,
    retirement_reason: &'static str,
}

impl QueuedSubmissionCommit {
    pub(crate) fn new(
        gate: QueueSubmissionGate,
        sequence: u64,
        retirement: CancellationToken,
        retirement_reason: &'static str,
    ) -> Self {
        Self {
            gate,
            sequence,
            retirement,
            retirement_reason,
        }
    }

    /// Starts the submission through the gate, or returns why the line
    /// refused it.
    pub(crate) fn commit<T>(&self, submit: impl FnOnce() -> T) -> Result<T, String> {
        self.gate.commit(
            self.sequence,
            &self.retirement,
            self.retirement_reason,
            submit,
        )
    }
}

#[cfg(test)]
#[path = "provider_queue_submission_gate_test.rs"]
mod tests;
