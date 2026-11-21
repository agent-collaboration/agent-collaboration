//! The Router's live operations and each binding's unknown outcomes (D1
//! state machines, design note §13.4, §14.1–§15.3). An operation is in the
//! index from its reservation, before its durable insert, so a reconcile
//! never finds a durable row whose live owner wasn't reserved. Admission
//! decides on these cells, not on a store scan held under the store lock:
//! the binding's uncertainty cell says whether an earlier send's outcome is
//! unknown, and the live operations say which sends are still in flight.
//! Every lock here is short and never held across an await.

use crate::binding_dispatch_permit::{BindingOutcomes, BindingUncertainty};
use collaboration_protocol::{
    ConversationOperationFailure, ConversationOperationSettlement, EndpointRef, OperationId,
    ProviderOperationKind, SessionRef,
};
use provider_session_machines::operation_record_machine::{
    AttemptEnding, DispatchAttempt, OperationRecord, OperationRecordEvent, OperationRecordMachine,
};
use state_machine_core::StateMachine;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard, PoisonError};
use tokio::sync::{Mutex, Notify};

#[derive(Default)]
pub(super) struct LiveOperations {
    pub(super) operations: HashMap<OperationId, Arc<LiveOperation>>,
    pub(super) terminal_order: VecDeque<OperationId>,
    /// Results whose durable write failed, in their own bounded window
    /// (design note §12.5): later settlements never evict them.
    pub(super) persistence_failed_order: VecDeque<OperationId>,
}

/// One operation's live owner: its result once published, its binding and
/// target, and its record on the operation record machine (design note
/// §11.5, §12.5): reserving, admitted, may-have-dispatched under one
/// attempt, settling an unknown outcome, settled.
pub(super) struct LiveOperation {
    pub(super) result:
        Mutex<Option<Result<ConversationOperationSettlement, ConversationOperationFailure>>>,
    /// Set once the result is published. Retention reads it without the result lock, which
    /// a reader of the result may be holding.
    pub(super) published: AtomicBool,
    pub(super) changed: Notify,
    /// The binding it runs on; none for an operation a test holds.
    binding: Option<EndpointRef>,
    kind: Option<ProviderOperationKind>,
    target: StdMutex<Option<SessionRef>>,
    record: StdMutex<OperationRecord>,
}

impl LiveOperation {
    /// An operation held live with no binding, as a test does.
    #[cfg(test)]
    pub(super) fn pending() -> Self {
        Self::new(
            None,
            None,
            None,
            OperationRecord::Admitted { queued: false },
        )
    }

    /// An operation reserved on `binding`, before its durable insert.
    pub(super) fn reserving(
        binding: EndpointRef,
        target: Option<SessionRef>,
        kind: ProviderOperationKind,
    ) -> Self {
        Self::new(
            Some(binding),
            target,
            Some(kind),
            OperationRecord::Reserving,
        )
    }

    pub(super) fn queued_prompt(target: SessionRef) -> Self {
        Self::new(
            Some(target.endpoint.clone()),
            Some(target),
            Some(ProviderOperationKind::ConversationPrompt),
            OperationRecord::Admitted { queued: true },
        )
    }

    pub(super) fn is_prompt_for(&self, target: &SessionRef) -> bool {
        self.kind == Some(ProviderOperationKind::ConversationPrompt)
            && self.target().as_ref() == Some(target)
    }

    fn new(
        binding: Option<EndpointRef>,
        target: Option<SessionRef>,
        kind: Option<ProviderOperationKind>,
        record: OperationRecord,
    ) -> Self {
        Self {
            result: Mutex::new(None),
            published: AtomicBool::new(false),
            changed: Notify::new(),
            binding,
            kind,
            target: StdMutex::new(target),
            record: StdMutex::new(record),
        }
    }

    /// Applies one event to the operation's record. A move the record no
    /// longer allows (a stale attempt, an operation already settled) changes
    /// nothing.
    fn advance(&self, event: OperationRecordEvent) -> bool {
        let mut record = lock(&self.record);
        match OperationRecordMachine::transition(&record, event) {
            Ok((next, _effects)) => {
                *record = next;
                true
            }
            Err(_) => false,
        }
    }

    /// The operation's record now.
    pub(super) fn record(&self) -> OperationRecord {
        *lock(&self.record)
    }

    pub(super) fn binding(&self) -> Option<&EndpointRef> {
        self.binding.as_ref()
    }

    pub(super) fn is_restore(&self) -> bool {
        self.kind.is_some_and(ProviderOperationKind::is_restore)
    }

    pub(super) fn target(&self) -> Option<SessionRef> {
        lock(&self.target).clone()
    }

    /// The durable insert answered with a row of its own: the reservation
    /// is now an admitted operation.
    pub(super) fn admitted(&self) {
        self.advance(OperationRecordEvent::DurableInserted);
        self.changed.notify_waiters();
    }

    /// The reservation ended without a row of its own: a durable row already
    /// existed (`conflict`), or the insert failed.
    pub(super) fn dropped(&self, conflict: bool) {
        self.advance(if conflict {
            OperationRecordEvent::DurableConflict
        } else {
            OperationRecordEvent::DurableInsertFailed
        });
        self.changed.notify_waiters();
    }

    pub(super) fn is_reserving(&self) -> bool {
        matches!(self.record(), OperationRecord::Reserving)
    }

    /// Its dispatch marker is being written: a new attempt may leave.
    pub(super) fn dispatch_marked(&self) {
        self.advance(OperationRecordEvent::DispatchMarked {
            attempt: DispatchAttempt(uuid::Uuid::now_v7()),
        });
    }

    /// Its dispatch marker was undone: nothing left under that attempt.
    #[cfg(test)]
    pub(super) fn undispatched(&self) {
        if let OperationRecord::MayHaveDispatched { attempt } = self.record() {
            self.advance(OperationRecordEvent::Undispatched { attempt });
        }
    }

    /// The operation's outcome: `ending` for an attempt that may have left,
    /// not sent for one that never did. An unknown ending stays live
    /// (SettlingUnknown) until `uncertainty_recorded`.
    pub(super) fn completed(&self, ending: AttemptEnding) {
        match self.record() {
            OperationRecord::MayHaveDispatched { attempt } => {
                self.advance(OperationRecordEvent::Completed { attempt, ending });
            }
            OperationRecord::Admitted { .. } => {
                self.advance(OperationRecordEvent::NotSent);
            }
            OperationRecord::Reserving
            | OperationRecord::Dropped
            | OperationRecord::SettlingUnknown { .. }
            | OperationRecord::Settled { .. } => {}
        }
    }

    /// Its unknown outcome is on the binding's uncertainty cell: it stops
    /// counting as live.
    pub(super) fn uncertainty_recorded(&self) {
        if let OperationRecord::SettlingUnknown { attempt } = self.record() {
            self.advance(OperationRecordEvent::UncertaintyRecorded { attempt });
        }
    }

    /// A send in handoff custody or already marked, with no published result.
    fn in_flight(&self) -> bool {
        let record = self.record();
        (matches!(
            record,
            OperationRecord::MayHaveDispatched { .. } | OperationRecord::SettlingUnknown { .. }
        ) || matches!(record, OperationRecord::Admitted { queued: false })
            && self
                .kind
                .is_some_and(|kind| kind != ProviderOperationKind::ConversationPrompt))
            && !self.published.load(Ordering::Acquire)
    }
}

/// A held operation's result, for a test that locks it as a reader would.
#[cfg(test)]
pub(crate) struct HeldResult(pub(super) Arc<LiveOperation>);

#[cfg(test)]
impl HeldResult {
    /// Takes the result lock now, without waiting, or `None` when it is taken.
    pub(crate) fn try_hold(&self) -> Option<impl Sized + '_> {
        self.0.result.try_lock().ok()
    }
}

impl LiveOperations {
    /// Sends still in flight on `binding` that an admission on `target`
    /// waits behind: the target's own, and binding-wide ones (no target).
    pub(super) fn in_flight_on(
        &self,
        binding: &EndpointRef,
        target: Option<&SessionRef>,
    ) -> Vec<OperationId> {
        self.operations
            .iter()
            .filter(|(_, operation)| {
                operation.binding() == Some(binding)
                    && operation.in_flight()
                    && match (operation.target(), target) {
                        (None, _) => true,
                        (Some(held), Some(asking)) => &held == asking,
                        (Some(_), None) => false,
                    }
            })
            .map(|(operation_id, _)| operation_id.clone())
            .collect()
    }
}

/// Every binding's uncertainty cell (design note §14.1).
#[derive(Default)]
pub(super) struct UncertaintyCells {
    cells: StdMutex<HashMap<EndpointRef, Arc<BindingUncertainty>>>,
}

impl UncertaintyCells {
    pub(super) fn slot(&self, binding: &EndpointRef) -> Arc<BindingUncertainty> {
        lock(&self.cells)
            .entry(binding.clone())
            .or_default()
            .clone()
    }

    /// The binding's cell, seeded on first use with `seed`'s rows.
    pub(super) async fn cell<F, Fut, E>(
        &self,
        binding: &EndpointRef,
        seed: F,
    ) -> Result<Arc<BindingUncertainty>, E>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<BindingOutcomes, E>>,
    {
        let cell = self.slot(binding);
        cell.outcomes
            .get_or_try_init(|| async { seed().await.map(StdMutex::new) })
            .await?;
        Ok(cell)
    }
}

fn lock<T>(mutex: &StdMutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
#[path = "provider_operation_index_test.rs"]
mod tests;
