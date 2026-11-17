//! The Router's live operations and each binding's unknown outcomes (D1
//! state machines, design note §13.4, §14.1–§15.3). An operation is in the
//! index from its reservation, before its durable insert, so a reconcile
//! never finds a durable row whose live owner wasn't reserved. Admission
//! decides on these cells, not on a store scan held under the store lock:
//! the binding's uncertainty cell says whether an earlier send's outcome is
//! unknown, and the live operations say which sends are still in flight.
//! Every lock here is short and never held across an await.

use crate::uncertainty_cell_machine::{
    UncertaintyCellMachine, UncertaintyEvent, UncertaintyRefusal, UnknownOutcomes, seeded,
};
use collaboration_protocol::{
    ConversationOperationFailure, ConversationOperationSettlement, EndpointRef, OperationId,
    SessionRef,
};
use state_machine_core::{StateMachine, TransitionError};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard, PoisonError};
use tokio::sync::{Mutex, Notify};

#[derive(Default)]
pub(super) struct LiveOperations {
    pub(super) operations: HashMap<OperationId, Arc<LiveOperation>>,
    pub(super) terminal_order: VecDeque<OperationId>,
}

/// One operation's live owner: its result once published, and while it is
/// live, its binding, its target and whether its send may have left.
pub(super) struct LiveOperation {
    pub(super) result:
        Mutex<Option<Result<ConversationOperationSettlement, ConversationOperationFailure>>>,
    /// Set once the result is published. Retention reads it without the result lock, which
    /// a reader of the result may be holding.
    pub(super) published: AtomicBool,
    pub(super) changed: Notify,
    /// The binding it runs on; none for an operation a test holds.
    binding: Option<EndpointRef>,
    target: StdMutex<Option<SessionRef>>,
    /// Marked may-have-dispatched and not undone.
    dispatched: AtomicBool,
    /// Reserved, its durable insert not yet answered.
    reserving: AtomicBool,
}

impl LiveOperation {
    /// An operation held live with no binding, as a test does.
    #[cfg(test)]
    pub(super) fn pending() -> Self {
        Self::new(None, None, false)
    }

    /// An operation reserved on `binding`, before its durable insert.
    pub(super) fn reserving(binding: EndpointRef, target: Option<SessionRef>) -> Self {
        Self::new(Some(binding), target, true)
    }

    fn new(binding: Option<EndpointRef>, target: Option<SessionRef>, reserving: bool) -> Self {
        Self {
            result: Mutex::new(None),
            published: AtomicBool::new(false),
            changed: Notify::new(),
            binding,
            target: StdMutex::new(target),
            dispatched: AtomicBool::new(false),
            reserving: AtomicBool::new(reserving),
        }
    }

    pub(super) fn binding(&self) -> Option<&EndpointRef> {
        self.binding.as_ref()
    }

    pub(super) fn target(&self) -> Option<SessionRef> {
        lock(&self.target).clone()
    }

    /// The durable insert answered: the reservation is now an admitted
    /// operation.
    pub(super) fn admitted(&self) {
        self.reserving.store(false, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    pub(super) fn is_reserving(&self) -> bool {
        self.reserving.load(Ordering::SeqCst)
    }

    pub(super) fn set_dispatched(&self, dispatched: bool) {
        self.dispatched.store(dispatched, Ordering::SeqCst);
    }

    /// A send that may have left and whose result isn't published yet.
    fn in_flight(&self) -> bool {
        self.dispatched.load(Ordering::SeqCst) && !self.published.load(Ordering::Acquire)
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

/// One binding's uncertainty cell, seeded once from the durable rows before
/// its first use.
type BindingUncertainty = tokio::sync::OnceCell<StdMutex<UnknownOutcomes>>;

/// Every binding's uncertainty cell (design note §14.1).
#[derive(Default)]
pub(super) struct UncertaintyCells {
    cells: StdMutex<HashMap<EndpointRef, Arc<BindingUncertainty>>>,
}

impl UncertaintyCells {
    /// The binding's cell, seeded on first use with `seed`'s rows.
    pub(super) async fn cell<F, Fut, E>(
        &self,
        binding: &EndpointRef,
        seed: F,
    ) -> Result<Arc<BindingUncertainty>, E>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<(OperationId, Option<SessionRef>)>, E>>,
    {
        let cell = Arc::clone(
            lock(&self.cells)
                .entry(binding.clone())
                .or_insert_with(|| Arc::new(BindingUncertainty::new())),
        );
        cell.get_or_try_init(|| async {
            let rows = seed().await?;
            Ok::<_, E>(StdMutex::new(seeded(
                rows.iter()
                    .map(|(operation, target)| (operation, target.as_ref())),
            )))
        })
        .await?;
        Ok(cell)
    }
}

/// Applies one event to a seeded binding cell under its lock.
pub(super) fn apply_uncertainty(
    cell: &BindingUncertainty,
    event: UncertaintyEvent,
) -> Result<(), UncertaintyRefusal> {
    let Some(outcomes) = cell.get() else {
        return Ok(());
    };
    let mut outcomes = lock(outcomes);
    match UncertaintyCellMachine::transition(&outcomes, event) {
        Ok((next, _effects)) => {
            *outcomes = next;
            Ok(())
        }
        Err(TransitionError::Domain(refusal)) => Err(refusal),
        // A reconcile of an operation the cell never held changes nothing.
        Err(TransitionError::NotAllowed { .. } | TransitionError::Stale) => Ok(()),
    }
}

fn lock<T>(mutex: &StdMutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
