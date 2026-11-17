//! The FIFO's waiting messages and who settles each (D1 state machines,
//! design note §12.2). A message's record is its settlement token: whoever
//! takes it out settles the message, exactly once. The worker takes it when
//! it settles a message itself or when the message's send is committed; the
//! session cell's moves that the Host didn't drive (a close, a failed load,
//! a stop, a closed connection) reach this sink and settle it not sent.

use crate::ExternalProviderRuntime;
use crate::ExternalProviderSupervisor;
use crate::provider_acp_message_fifo::ProviderQueuedPrompt;
use collaboration_protocol::{OperationId, SessionRef};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// The phrase the session cell ends a failed load's messages with; the
/// worker replaces it with the load's own reason when it has one.
const LOAD_FAILED: &str = "the session couldn't be loaded";

/// One waiting message: its session, the connection whose line holds it,
/// and what to send.
#[derive(Clone)]
pub(crate) struct QueuedRecord {
    pub(crate) target: SessionRef,
    pub(crate) runtime: Arc<ExternalProviderRuntime>,
    pub(crate) prompt: ProviderQueuedPrompt,
    /// A delivery admitted when it reached the head (an API prompt is
    /// admitted before it is queued); it dispatches as a delivery.
    pub(crate) admitted_at_head: bool,
}

#[derive(Default)]
struct EntriesState {
    records: HashMap<String, QueuedRecord>,
    /// Messages a move ended before their door handed over the payload; the
    /// door settles them with this reason.
    ended_early: HashMap<String, &'static str>,
    /// Messages a failed load ended, waiting for the worker to settle them
    /// with the load's reason.
    load_failed: HashMap<String, QueuedRecord>,
}

pub(crate) struct QueuedEntries {
    /// Weak: provider runtimes hold this sink, and the supervisor owns them.
    supervisor: std::sync::Weak<ExternalProviderSupervisor>,
    state: Mutex<EntriesState>,
}

impl QueuedEntries {
    pub(crate) fn new(supervisor: &Arc<ExternalProviderSupervisor>) -> Self {
        Self {
            supervisor: Arc::downgrade(supervisor),
            state: Mutex::new(EntriesState::default()),
        }
    }

    /// The door hands over a message's payload. A message a move already
    /// ended settles at once and is not queued; returns whether it was.
    pub(crate) fn insert(&self, record: QueuedRecord) -> bool {
        let key = String::from(record.prompt.operation_id().clone());
        let ended = {
            let mut state = self.lock();
            match state.ended_early.remove(&key) {
                Some(reason) => Some(reason),
                None => {
                    state.records.insert(key, record.clone());
                    None
                }
            }
        };
        match ended {
            Some(reason) => {
                self.settle(&record.prompt, reason);
                false
            }
            None => true,
        }
    }

    /// A copy of the message's record while it waits.
    pub(crate) fn get(&self, operation_id: &OperationId) -> Option<QueuedRecord> {
        self.lock()
            .records
            .get(&String::from(operation_id.clone()))
            .cloned()
    }

    /// Records a waiting delivery's admitted operation, so whoever settles
    /// the message settles that operation too. False when the message ended
    /// meanwhile.
    pub(crate) fn admit(
        &self,
        operation_id: &OperationId,
        admitted: crate::external_provider_supervisor::AdmittedApiPrompt,
    ) -> bool {
        let mut state = self.lock();
        match state.records.get_mut(&String::from(operation_id.clone())) {
            Some(QueuedRecord {
                prompt: ProviderQueuedPrompt::Contents { admitted: slot, .. },
                admitted_at_head,
                ..
            }) => {
                *slot = Some(admitted);
                *admitted_at_head = true;
                true
            }
            None => false,
        }
    }

    /// Takes the message's settlement: the caller settles it from here on.
    pub(crate) fn take(&self, operation_id: &OperationId) -> Option<QueuedRecord> {
        self.lock()
            .records
            .remove(&String::from(operation_id.clone()))
    }

    /// Settles a message the worker ended, if it still waits.
    pub(crate) fn settle_not_sent(&self, operation_id: &OperationId, reason: &str) {
        if let Some(record) = self.take(operation_id) {
            self.settle(&record.prompt, reason);
        }
    }

    /// Settles a message its failed load ended, with the load's reason.
    pub(crate) fn settle_load_failed(&self, operation_id: &OperationId, reason: &str) {
        let record = self
            .lock()
            .load_failed
            .remove(&String::from(operation_id.clone()));
        if let Some(record) = record {
            self.settle(&record.prompt, reason);
        }
    }

    /// Every message of `target` still to settle, for a worker that stops.
    pub(crate) fn waiting_for(&self, target: &SessionRef) -> Vec<OperationId> {
        let state = self.lock();
        state
            .records
            .values()
            .chain(state.load_failed.values())
            .filter(|record| &record.target == target)
            .map(|record| record.prompt.operation_id().clone())
            .collect()
    }

    /// Settles a message whatever holds it: waiting, or ended by a failed
    /// load and not yet settled.
    pub(crate) fn settle_any_not_sent(&self, operation_id: &OperationId, reason: &str) {
        let key = String::from(operation_id.clone());
        let record = {
            let mut state = self.lock();
            state
                .records
                .remove(&key)
                .or_else(|| state.load_failed.remove(&key))
        };
        if let Some(record) = record {
            self.settle(&record.prompt, reason);
        }
    }

    /// Settles a message not sent, while the supervisor is still running.
    fn settle(&self, prompt: &ProviderQueuedPrompt, reason: impl Into<String>) {
        if let Some(supervisor) = self.supervisor.upgrade() {
            mark_entry_not_sent(&supervisor, prompt, reason);
        }
    }

    fn lock(&self) -> MutexGuard<'_, EntriesState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl acp_client_runtime::QueueEntrySink for QueuedEntries {
    fn entry_not_sent(&self, _session_id: &str, operation_id: &str, reason: &'static str) {
        let record = {
            let mut state = self.lock();
            match state.records.remove(operation_id) {
                Some(record) if reason == LOAD_FAILED => {
                    // The worker that awaited this load settles it with the
                    // load's own reason.
                    state.load_failed.insert(operation_id.to_owned(), record);
                    None
                }
                Some(record) => Some(record),
                None => {
                    state.ended_early.insert(operation_id.to_owned(), reason);
                    None
                }
            }
        };
        if let Some(record) = record {
            self.settle(&record.prompt, reason);
        }
    }
}

/// A message leaves the line unsent: the registry says so, and an admitted
/// API prompt's operation is settled by its one completion owner.
pub(crate) fn mark_entry_not_sent(
    supervisor: &ExternalProviderSupervisor,
    entry: &ProviderQueuedPrompt,
    reason: impl Into<String>,
) {
    let reason = reason.into();
    let ProviderQueuedPrompt::Contents {
        request, admitted, ..
    } = entry;
    // A claimed API entry is being dispatched by its own owner, which settles
    // it with its real outcome; the line must not call it unsent (D1 R14).
    if admitted
        .as_ref()
        .is_some_and(|admitted| admitted.is_settled())
    {
        return;
    }
    supervisor
        .queued_operation_registry()
        .mark_not_submitted(&request.operation_id, reason.clone());
    if let Some(admitted) = admitted {
        supervisor.complete_unsent_api_entry(
            admitted,
            &request.operation_id,
            &request.target,
            &reason,
        );
    }
}
