//! The Router's live operations and each binding's unknown outcomes (D1
//! state machines, design note §13.4, §14.1–§15.3). An operation is in the
//! index from its reservation, before its durable insert, so a reconcile
//! never finds a durable row whose live owner wasn't reserved. Admission
//! decides on these cells, not on a store scan held under the store lock:
//! the binding's uncertainty cell says whether an earlier send's outcome is
//! unknown, and the live operations say which sends are still in flight.
//! Every lock here is short and never held across an await.

use super::{
    MAX_RETAINED_SETTLEMENTS,
    provider_admission_policy::{
        AdmissionBlockers, AdmissionRequest, DispatchMark, EarlierOperation,
    },
    provider_operation_finish::Retention,
};
use crate::binding_dispatch_permit::{BindingOutcomes, BindingUncertainty};
use crate::provider_operation_custody::{
    CancelledPromptIdentity, CustodyEnd, LaneWriteCustody, LatePromptRepairAuthority,
    OperationLifecycle,
};
use collaboration_protocol::{
    CodexGeneration, ConversationOperationFailure, ConversationOperationSettlement, EndpointRef,
    OperationId, ProviderBindingIdentity, ProviderOperationKind, SessionRef,
};
use provider_session_machines::operation_record_machine::{
    AttemptEnding, DispatchAttempt, OperationRecord, OperationRecordEvent, OperationRecordMachine,
};
use state_machine_core::StateMachine;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard, PoisonError};
use tokio::sync::{Mutex, Notify};

#[derive(Default)]
pub(super) struct LiveOperations {
    pub(super) operations: HashMap<OperationId, Arc<LiveOperation>>,
    terminal_order: VecDeque<OperationId>,
    /// Results whose durable write failed, in their own bounded window
    /// (design note §12.5): later settlements never evict them.
    persistence_failed_order: VecDeque<OperationId>,
}

pub(super) enum Reservation {
    Acquired,
    Occupied(Arc<LiveOperation>),
}

/// One operation's live owner: its result once published, its binding and
/// target, and its record on the operation record machine (design note
/// §11.5, §12.5): reserving, admitted, may-have-dispatched under one
/// attempt, settling an unknown outcome, settled.
pub(super) struct LiveOperation {
    pub(super) result:
        Mutex<Option<Result<ConversationOperationSettlement, ConversationOperationFailure>>>,
    pub(super) changed: Arc<Notify>,
    /// The binding it runs on; none for an operation a test holds.
    binding: Option<(EndpointRef, CodexGeneration)>,
    kind: Option<ProviderOperationKind>,
    mark: DispatchMark,
    target: StdMutex<Option<SessionRef>>,
    lifecycle: Arc<OperationLifecycle>,
}

impl LiveOperation {
    /// An operation held live with no binding, as a test does.
    #[cfg(test)]
    pub(super) fn pending() -> Self {
        Self::new(
            None,
            None,
            None,
            DispatchMark::Deferred,
            OperationRecord::Admitted { queued: false },
        )
    }

    /// An operation reserved on `binding`, before its durable insert.
    pub(super) fn reserving(
        binding: &ProviderBindingIdentity,
        target: Option<SessionRef>,
        kind: ProviderOperationKind,
        mark: DispatchMark,
    ) -> Self {
        Self::new(
            Some(binding),
            target,
            Some(kind),
            mark,
            OperationRecord::Reserving,
        )
    }

    pub(super) fn queued_prompt(binding: &ProviderBindingIdentity, target: SessionRef) -> Self {
        Self::new(
            Some(binding),
            Some(target),
            Some(ProviderOperationKind::ConversationPrompt),
            DispatchMark::Deferred,
            OperationRecord::Admitted { queued: true },
        )
    }

    pub(super) fn is_prompt_for(&self, target: &SessionRef) -> bool {
        self.kind == Some(ProviderOperationKind::ConversationPrompt)
            && self.target().as_ref() == Some(target)
    }

    fn new(
        binding: Option<&ProviderBindingIdentity>,
        target: Option<SessionRef>,
        kind: Option<ProviderOperationKind>,
        mark: DispatchMark,
        record: OperationRecord,
    ) -> Self {
        let changed = Arc::new(Notify::new());
        Self {
            result: Mutex::new(None),
            changed: Arc::clone(&changed),
            binding: binding.map(|binding| (binding.endpoint.clone(), binding.generation.clone())),
            kind,
            mark,
            target: StdMutex::new(target),
            lifecycle: Arc::new(OperationLifecycle::new(record, changed)),
        }
    }

    /// Applies one event to the operation's record. A move the record no
    /// longer allows (a stale attempt, an operation already settled) changes
    /// nothing.
    fn advance(&self, event: OperationRecordEvent) -> bool {
        let mut lifecycle = self.lifecycle.state();
        let previous = lifecycle.record;
        match OperationRecordMachine::transition(&previous, event) {
            Ok((next, _effects)) => {
                lifecycle.custody.record_moved(previous, next);
                lifecycle.record = next;
                true
            }
            Err(_) => false,
        }
    }

    /// The operation's record now.
    pub(super) fn record(&self) -> OperationRecord {
        self.lifecycle.state().record
    }

    pub(super) fn holds_custody(&self) -> bool {
        self.lifecycle.state().custody.holds_custody()
    }

    pub(super) fn finish_write(&self, reason: CustodyEnd) -> bool {
        self.lifecycle.finish_host(reason)
    }

    pub(super) fn begin_lane_write(
        &self,
        generation: &CodexGeneration,
        target: &SessionRef,
        on_end: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<LaneWriteCustody> {
        if self.matches_lane_binding(generation, target) {
            self.lifecycle.begin_lane_write(on_end)
        } else {
            None
        }
    }

    pub(super) fn authorize_late_prompt_repair(
        &self,
        generation: &CodexGeneration,
        target: &SessionRef,
        identity: &CancelledPromptIdentity,
    ) -> Option<LatePromptRepairAuthority> {
        if self.kind != Some(ProviderOperationKind::ConversationPrompt)
            || !self.matches_lane_binding(generation, target)
        {
            return None;
        }
        self.lifecycle
            .authorize_late_prompt_repair(generation.clone(), identity)
    }

    fn matches_lane_binding(&self, generation: &CodexGeneration, target: &SessionRef) -> bool {
        self.binding
            .as_ref()
            .is_some_and(|(endpoint, held_generation)| {
                endpoint == &target.endpoint && held_generation == generation
            })
            && self.target().as_ref() == Some(target)
    }

    pub(super) async fn wait_for_result(
        &self,
    ) -> Option<Result<ConversationOperationSettlement, ConversationOperationFailure>> {
        loop {
            let changed = self.changed.notified();
            if let Some(result) = self.result.lock().await.clone() {
                return Some(result);
            }
            if matches!(self.record(), OperationRecord::Dropped) {
                return None;
            }
            changed.await;
        }
    }

    pub(super) fn binding(&self) -> Option<&EndpointRef> {
        self.binding.as_ref().map(|(endpoint, _)| endpoint)
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
    pub(super) fn dropped(&self, conflict: bool, end: CustodyEnd) {
        self.advance(if conflict {
            OperationRecordEvent::DurableConflict
        } else {
            OperationRecordEvent::DurableInsertFailed
        });
        self.finish_write(end);
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

    /// Its outcome is on the binding's uncertainty cell. Its writer may
    /// still hold custody of the operation row.
    pub(super) fn uncertainty_recorded(&self) {
        if let OperationRecord::SettlingUnknown { attempt } = self.record() {
            self.advance(OperationRecordEvent::UncertaintyRecorded { attempt });
        }
    }

    /// An admitted handoff or marked send whose writer still holds custody.
    fn in_flight(&self) -> bool {
        let lifecycle = self.lifecycle.state();
        let record = lifecycle.custody.admission_record(lifecycle.record);
        lifecycle.custody.holds_custody()
            && (record == OperationRecord::Reserving && self.mark == DispatchMark::BeforeHandoff
                || matches!(
                    record,
                    OperationRecord::MayHaveDispatched { .. }
                        | OperationRecord::SettlingUnknown { .. }
                )
                || matches!(record, OperationRecord::Admitted { queued: false })
                    && self
                        .kind
                        .is_some_and(|kind| kind != ProviderOperationKind::ConversationPrompt))
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
    pub(super) fn holds_custody_of(&self, operation: &OperationId) -> bool {
        self.operations
            .get(operation)
            .is_some_and(|live| live.holds_custody())
    }

    pub(super) fn custodians(&self) -> impl Iterator<Item = (&OperationId, &Arc<LiveOperation>)> {
        self.operations
            .iter()
            .filter(|(_, live)| live.holds_custody())
    }

    pub(super) fn reserve(
        &mut self,
        binding: &ProviderBindingIdentity,
        request: AdmissionRequest<'_>,
        live: &Arc<LiveOperation>,
        mut unknown: Vec<OperationId>,
        unconfirmed_cancel: Option<&str>,
    ) -> Result<Reservation, AdmissionBlockers> {
        if let Some(prior) = self.operations.get(request.operation) {
            return Ok(Reservation::Occupied(Arc::clone(prior)));
        }
        unknown.retain(|operation| {
            request.blocked_by(operation, EarlierOperation::Unknown, unconfirmed_cancel)
        });
        let in_flight = self.in_flight_blockers(binding, request);
        if !unknown.is_empty() || !in_flight.is_empty() {
            return Err(AdmissionBlockers { unknown, in_flight });
        }
        self.operations
            .insert(request.operation.clone(), Arc::clone(live));
        Ok(Reservation::Acquired)
    }

    pub(super) fn begin_lane_write(
        &self,
        operation: &OperationId,
        generation: &CodexGeneration,
        target: &SessionRef,
        on_end: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<LaneWriteCustody> {
        self.operations
            .get(operation)?
            .begin_lane_write(generation, target, on_end)
    }

    pub(super) fn retain_result(&mut self, operation: OperationId, retention: Retention) {
        let order = match retention {
            Retention::Settled => &mut self.terminal_order,
            Retention::PersistenceFailed => &mut self.persistence_failed_order,
        };
        order.push_back(operation);
        Self::trim_window(order, &mut self.operations);
    }

    pub(super) fn trim_results(&mut self) {
        Self::trim_window(&mut self.terminal_order, &mut self.operations);
        Self::trim_window(&mut self.persistence_failed_order, &mut self.operations);
    }

    fn trim_window(
        order: &mut VecDeque<OperationId>,
        operations: &mut HashMap<OperationId, Arc<LiveOperation>>,
    ) {
        if order.len() <= MAX_RETAINED_SETTLEMENTS {
            return;
        }
        // Held entries do not count against the bounded window of ended results.
        let ended = order
            .iter()
            .filter(|operation| {
                operations
                    .get(*operation)
                    .is_some_and(|live| !live.holds_custody())
            })
            .count();
        let mut excess = ended.saturating_sub(MAX_RETAINED_SETTLEMENTS);
        order.retain(|operation| {
            if excess != 0
                && operations
                    .get(operation)
                    .is_some_and(|live| !live.holds_custody())
            {
                excess -= 1;
                operations.remove(operation);
                false
            } else {
                true
            }
        });
    }

    /// Sends still in flight on `binding` that an admission on `target`
    /// waits behind: the target's own, and binding-wide ones (no target).
    pub(super) fn in_flight_blockers(
        &self,
        binding: &ProviderBindingIdentity,
        request: AdmissionRequest<'_>,
    ) -> Vec<OperationId> {
        self.operations
            .iter()
            .filter(|(id, operation)| {
                operation
                    .binding
                    .as_ref()
                    .is_some_and(|(endpoint, generation)| {
                        endpoint == &binding.endpoint && generation == &binding.generation
                    })
                    && operation.in_flight()
                    && request.blocked_by(
                        id,
                        EarlierOperation::InFlight {
                            target: operation.target().as_ref(),
                        },
                        None,
                    )
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
