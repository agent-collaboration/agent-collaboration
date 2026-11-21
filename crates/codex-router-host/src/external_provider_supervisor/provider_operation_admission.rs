//! One admission per operation ID (D1 Change 4 and Change 5; state machines,
//! design note §14.1–§15.3): a reused ID must match its request, an earlier
//! send's unknown outcome blocks the session, and a new row is persisted
//! before anything reaches the agent. Decisions read the binding's cells.

use super::{
    Arc, ConversationBindingIdentity, ConversationOperationFailure,
    ConversationOperationFailureKind, ConversationOperationFailureStage,
    ExternalProviderSupervisor, LiveOperation, OperationId, PreparedOperation,
    ProviderBindingIdentity, ProviderOperationAdmission, ProviderOperationAdmissionResult,
    ProviderOperationEffect, ProviderOperationKind, ProviderOperationRecord,
    ProviderReconciliationState, SessionRef, admission_failure, failure, snapshot_failure,
    snapshot_from_record, unavailable_failure,
};
use crate::provider_operation_settlement::ProviderOperationCompletion;
use router_clock::ClockInstant;
use std::{future::Future, time::Duration};
use tokio::sync::oneshot;

const HOST_ADMISSION_BUDGET: Duration = Duration::from_secs(10);

struct AdmissionWrite {
    operation: ProviderOperationAdmission,
    target: Option<SessionRef>,
    deadline: ClockInstant,
    live: Arc<LiveOperation>,
}

fn admission_expired(
    operation: &OperationId,
    target: Option<&SessionRef>,
) -> ConversationOperationFailure {
    failure(
        ConversationOperationFailureKind::Busy,
        ConversationOperationFailureStage::Admission,
        ProviderOperationEffect::None,
        "the Router was busy and nothing was sent, so send again as a new operation",
        operation.clone(),
        target.cloned(),
    )
}

fn dispatch_mark_expired(
    operation: &OperationId,
    live: &LiveOperation,
) -> ConversationOperationFailure {
    failure(
        ConversationOperationFailureKind::Unavailable,
        ConversationOperationFailureStage::Dispatch,
        ProviderOperationEffect::None,
        "the store didn't respond in time, so nothing was sent",
        operation.clone(),
        live.target(),
    )
}

/// When an admitted operation is marked as possibly dispatched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DispatchMark {
    /// A retained lifecycle operation marks before handing off to the owner.
    BeforeHandoff,
    /// Later, when the owner authorizes a prompt or steer to dispatch.
    Deferred,
}

/// What blocks an admission, by state: each state has its own next step.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct AdmissionBlockers {
    /// Earlier sends whose outcome is unknown: reconcile them first.
    pub(super) unknown: Vec<OperationId>,
    /// Sends still in flight on the same session (or binding-wide): wait
    /// for them.
    pub(super) in_flight: Vec<OperationId>,
}

impl AdmissionBlockers {
    /// The refusal's one line: each blocking state and its valid next step
    /// (D1 M8); none when nothing blocks.
    pub(super) fn answer(&self) -> Option<String> {
        let ids = |operations: &[OperationId]| {
            operations
                .iter()
                .map(OperationId::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut clauses = Vec::new();
        if !self.unknown.is_empty() {
            clauses.push(format!(
                "an earlier prompt's outcome is unknown; check the session, then run operation reconcile for {}",
                ids(&self.unknown)
            ));
        }
        if !self.in_flight.is_empty() {
            clauses.push(format!(
                "an earlier send is still in flight; wait for {}",
                ids(&self.in_flight)
            ));
        }
        (!clauses.is_empty()).then(|| clauses.join("; "))
    }
}

/// The refusal for an operation ID reused for a different request (R15).
const REUSED_ID_MESSAGE: &str = "request ID already used for a different request";

/// Whether a stored operation is the same request: its kind, and its target
/// when both sides know one (a create has no target yet).
pub(super) fn same_request(
    record: &ProviderOperationRecord,
    operation_kind: ProviderOperationKind,
    known_target: Option<&SessionRef>,
) -> bool {
    record.operation_kind == operation_kind
        && match (record.target.as_ref(), known_target) {
            (Some(stored), Some(requested)) => stored == requested,
            _ => true,
        }
}

/// The refusal for a request ID already used by a different request (D1 R15).
pub(super) fn reused_id_failure(
    operation_id: OperationId,
    target: Option<SessionRef>,
) -> ConversationOperationFailure {
    failure(
        ConversationOperationFailureKind::InvalidRequest,
        ConversationOperationFailureStage::Validation,
        ProviderOperationEffect::None,
        REUSED_ID_MESSAGE,
        operation_id,
        target,
    )
}

impl ExternalProviderSupervisor {
    pub(super) async fn run_operation_for_handoff(
        &self,
        operation_id: OperationId,
        live: Arc<LiveOperation>,
        operation: impl Future<Output = ProviderOperationCompletion>,
    ) {
        let deadline = self.admission_deadline();
        let clock = self.inner.clock.as_ref();
        let mark = self.mark_operation_for_handoff(&operation_id, &live, deadline);
        tokio::pin!(mark);
        let receipt = tokio::select! {
            biased;
            () = self.inner.late_dispatch_cleanup_cancel.cancelled() => Err(router_clock::WaitElapsed),
            receipt = router_clock::timeout_at(clock, deadline, &mut mark) => receipt,
        };
        let completion = match receipt {
            Ok(Ok(store)) if clock.monotonic_now() < deadline => {
                drop(store);
                live.dispatch_marked();
                operation.await
            }
            Ok(Err(failure)) if clock.monotonic_now() < deadline => {
                ProviderOperationCompletion::DispatchMarkNotWritten(failure)
            }
            late => {
                let failure = dispatch_mark_expired(&operation_id, &live);
                self.inner
                    .finish(
                        operation_id.clone(),
                        Arc::clone(&live),
                        ProviderOperationCompletion::DispatchMarkNotWritten(failure.clone()),
                    )
                    .await;
                // Keep custody of the SQL future, even though the caller's
                // authorization expired. A committed mark must be settled.
                let settle_late_mark = async {
                    if let Ok(mut store) = match late {
                        Ok(receipt) => receipt,
                        Err(_) => mark.await,
                    } {
                        self.inner
                            .settle_late_dispatch_mark(&mut store, &operation_id, &failure)
                            .await;
                    }
                };
                tokio::select! {
                    biased;
                    () = self.inner.late_dispatch_cleanup_cancel.cancelled() => {}
                    () = settle_late_mark => {}
                }
                return;
            }
        };
        self.inner
            .finish(operation_id.clone(), Arc::clone(&live), completion)
            .await;
    }

    #[expect(
        clippy::result_large_err,
        reason = "Retains the operation's typed failure."
    )]
    async fn mark_operation_for_handoff<'store>(
        &'store self,
        operation: &OperationId,
        live: &LiveOperation,
        deadline: ClockInstant,
    ) -> Result<
        tokio::sync::MutexGuard<'store, super::ProviderOperationStore>,
        ConversationOperationFailure,
    > {
        let clock = self.inner.clock.as_ref();
        let target = live.target();
        let not_sent = |message| {
            failure(
                ConversationOperationFailureKind::Unavailable,
                ConversationOperationFailureStage::Dispatch,
                ProviderOperationEffect::None,
                message,
                operation.clone(),
                target.clone(),
            )
        };
        let elapsed = || dispatch_mark_expired(operation, live);
        let acquire_store = async {
            #[cfg(test)]
            {
                let pause = self
                    .inner
                    .dispatch_mark_test_pause
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(pause) = pause {
                    let _test_ended = pause.entered.send(());
                    let _test_ended = pause.release.await;
                }
            }
            self.inner.store.lock().await
        };
        let mut store = router_clock::timeout_at(clock, deadline, acquire_store)
            .await
            .map_err(|_| elapsed())?;
        if clock.monotonic_now() >= deadline {
            return Err(elapsed());
        }
        let mark = store
            .mark_may_have_dispatched_if(operation, self.inner.now_ms(), || {
                clock.monotonic_now() < deadline
            })
            .await
            .map_err(|error| {
                tracing::warn!(operation = operation.as_str(), %error, "could not mark provider dispatch");
                not_sent("the store couldn't record the dispatch mark, so nothing was sent")
            })?;
        mark.ok_or_else(elapsed)?;
        Ok(store)
    }

    pub(super) fn admission_deadline(&self) -> ClockInstant {
        self.inner
            .clock
            .monotonic_now()
            .after(HOST_ADMISSION_BUDGET)
    }

    #[expect(
        clippy::result_large_err,
        reason = "Retains the typed admission refusal."
    )]
    pub(super) async fn within_admission<T>(
        &self,
        deadline: ClockInstant,
        operation: &OperationId,
        target: Option<&SessionRef>,
        step: impl Future<Output = T>,
    ) -> Result<T, ConversationOperationFailure> {
        let clock = self.inner.clock.as_ref();
        if clock.monotonic_now() >= deadline {
            return Err(admission_expired(operation, target));
        }
        router_clock::timeout_at(clock, deadline, step)
            .await
            .map_err(|_| admission_expired(operation, target))
    }

    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
    fn existing_operation(
        operation_id: OperationId,
        record: ProviderOperationRecord,
        operation_kind: ProviderOperationKind,
        known_target: Option<&SessionRef>,
    ) -> Result<PreparedOperation, ConversationOperationFailure> {
        if !same_request(&record, operation_kind, known_target) {
            return Err(reused_id_failure(operation_id, known_target.cloned()));
        }
        Ok(PreparedOperation::Existing(
            snapshot_from_record(record)
                .map_err(|message| snapshot_failure(operation_id, message))?,
        ))
    }

    /// What blocks an admission on `known_target`: an earlier send's unknown
    /// outcome with no live owner (the binding's uncertainty cell); and, for
    /// an operation dispatched at admission, a send still in flight on the
    /// same session (or binding-wide), as today's scan counted it.
    pub(super) async fn admission_blockers(
        &self,
        binding: &ProviderBindingIdentity,
        known_target: Option<&SessionRef>,
        mark: DispatchMark,
        operation_kind: ProviderOperationKind,
    ) -> Result<AdmissionBlockers, ()> {
        let cell = self.inner.binding_uncertainty(binding).await?;
        let can_recover_close = operation_kind.is_restore()
            || known_target.is_some_and(|target| {
                self.owner_session_activity(target) != crate::ProviderSessionActivity::NotLoaded
            });
        let mut unknown = crate::binding_dispatch_permit::lock(cell.outcomes.get().ok_or(())?)
            .blockers(known_target, can_recover_close);
        if operation_kind == ProviderOperationKind::ConversationClose
            && let Some(input) = known_target
                .and_then(|target| self.owner_session_snapshot(target))
                .and_then(|snapshot| snapshot.cancel_unconfirmed)
        {
            let foreground = input.operation.to_string();
            unknown.retain(|operation| operation.as_str() != foreground);
        }
        let in_flight = if mark == DispatchMark::BeforeHandoff
            && operation_kind != ProviderOperationKind::ConversationClose
        {
            self.inner
                .live_operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .in_flight_on(&binding.endpoint, known_target)
        } else {
            Vec::new()
        };
        Ok(AdmissionBlockers { unknown, in_flight })
    }

    #[expect(
        clippy::result_large_err,
        reason = "Keeps the operation identity and typed refusal."
    )]
    pub(super) async fn reserve_queued_delivery(
        &self,
        deadline: ClockInstant,
        operation_id: &OperationId,
        target: &SessionRef,
        binding: &ProviderBindingIdentity,
    ) -> Result<Option<Arc<LiveOperation>>, ConversationOperationFailure> {
        let kind = ProviderOperationKind::ConversationPrompt;
        let existing = self
            .within_admission(deadline, operation_id, Some(target), async {
                self.wait_for_reservation(operation_id).await;
                if let Some(record) = self
                    .stored_operation(operation_id, &Some(target.clone()))
                    .await?
                {
                    Self::existing_operation(operation_id.clone(), record, kind, Some(target))?;
                    return Ok(true);
                }
                self.check_operation_blockers(
                    operation_id,
                    kind,
                    binding,
                    Some(target),
                    DispatchMark::Deferred,
                )
                .await?;
                Ok(false)
            })
            .await??;
        if existing {
            return Ok(None);
        }
        let mut operations = self
            .inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.inner.clock.monotonic_now() >= deadline {
            return Err(admission_expired(operation_id, Some(target)));
        }
        match operations.operations.entry(operation_id.clone()) {
            std::collections::hash_map::Entry::Occupied(entry)
                if entry.get().is_prompt_for(target) =>
            {
                Ok(None)
            }
            std::collections::hash_map::Entry::Occupied(_) => Err(reused_id_failure(
                operation_id.clone(),
                Some(target.clone()),
            )),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let live = Arc::new(LiveOperation::queued_prompt(target.clone()));
                entry.insert(Arc::clone(&live));
                Ok(Some(live))
            }
        }
    }

    #[expect(
        clippy::result_large_err,
        reason = "Keeps the operation identity and typed refusal."
    )]
    async fn check_operation_blockers(
        &self,
        operation: &OperationId,
        kind: ProviderOperationKind,
        binding: &ProviderBindingIdentity,
        target: Option<&SessionRef>,
        mark: DispatchMark,
    ) -> Result<(), ConversationOperationFailure> {
        if kind == ProviderOperationKind::ConversationCancel {
            return Ok(());
        }
        let blocking = self
            .admission_blockers(binding, target, mark, kind)
            .await
            .map_err(|()| unavailable_failure(operation.clone(), target.cloned()))?;
        if let Some(answer) = blocking.answer() {
            return Err(failure(
                ConversationOperationFailureKind::Busy,
                ConversationOperationFailureStage::Validation,
                ProviderOperationEffect::None,
                &answer,
                operation.clone(),
                target.cloned(),
            ));
        }
        Ok(())
    }

    /// Owns a new reservation up to durable admission. Only the transaction
    /// finishing under the store lock can outlive the caller's deadline.
    #[expect(
        clippy::result_large_err,
        reason = "Retains the typed admission refusal."
    )]
    pub(super) async fn admit_operation(
        &self,
        deadline: ClockInstant,
        operation_id: OperationId,
        operation_kind: ProviderOperationKind,
        binding: ProviderBindingIdentity,
        known_target: Option<&SessionRef>,
        mark: DispatchMark,
    ) -> Result<PreparedOperation, ConversationOperationFailure> {
        let admitted_at_ms = self.inner.now_ms();
        let failure_target = known_target.cloned();
        let lookup = async {
            self.wait_for_reservation(&operation_id).await;
            self.stored_operation(&operation_id, &failure_target).await
        };
        if let Some(record) = self
            .within_admission(deadline, &operation_id, known_target, lookup)
            .await??
        {
            return Self::existing_operation(operation_id, record, operation_kind, known_target);
        }
        self.within_admission(
            deadline,
            &operation_id,
            known_target,
            self.check_operation_blockers(
                &operation_id,
                operation_kind,
                &binding,
                known_target,
                mark,
            ),
        )
        .await??;

        let live = Arc::new(LiveOperation::reserving(
            binding.endpoint.clone(),
            known_target.cloned(),
            operation_kind,
        ));
        let reserved = {
            use std::collections::hash_map::Entry;
            let mut operations = self
                .inner
                .live_operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if self.inner.clock.monotonic_now() >= deadline {
                return Err(admission_expired(&operation_id, known_target));
            }
            match operations.operations.entry(operation_id.clone()) {
                Entry::Occupied(_) => false,
                Entry::Vacant(entry) => {
                    entry.insert(Arc::clone(&live));
                    true
                }
            }
        };
        if !reserved {
            let lookup = async {
                self.wait_for_reservation(&operation_id).await;
                self.stored_operation(&operation_id, &failure_target).await
            };
            return match self
                .within_admission(deadline, &operation_id, known_target, lookup)
                .await??
            {
                Some(record) => {
                    Self::existing_operation(operation_id, record, operation_kind, known_target)
                }
                None => Err(unavailable_failure(operation_id, failure_target)),
            };
        }

        let write = AdmissionWrite {
            operation: ProviderOperationAdmission {
                operation_id: operation_id.clone(),
                operation_kind,
                binding: ConversationBindingIdentity::ExternalProvider { binding },
                admitted_at_ms,
            },
            target: failure_target,
            deadline,
            live,
        };
        let supervisor = self.clone();
        let (reply, completion) = oneshot::channel();
        let (accept, accepted) = oneshot::channel();
        self.inner.operation_tasks.spawn(async move {
            supervisor.complete_admission(write, reply, accepted).await;
        });
        let prepared = self
            .within_admission(deadline, &operation_id, known_target, completion)
            .await?
            .map_err(|_| unavailable_failure(operation_id, known_target.cloned()))??;
        let _admission_task_ended = accept.send(());
        Ok(prepared)
    }

    async fn complete_admission(
        &self,
        write: AdmissionWrite,
        reply: oneshot::Sender<Result<PreparedOperation, ConversationOperationFailure>>,
        accepted: oneshot::Receiver<()>,
    ) {
        let AdmissionWrite {
            operation,
            target,
            deadline,
            live,
        } = write;
        let operation_id = operation.operation_id.clone();
        let kind = operation.operation_kind;
        let target = target.as_ref();
        let clock = self.inner.clock.as_ref();
        let acquire_store = async {
            // Tests can hold the reservation before any admission SQL runs.
            #[cfg(test)]
            {
                let pause = self
                    .inner
                    .admission_test_pause
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(pause) = pause {
                    let _result = pause.entered.send(());
                    let _result = pause.release.await;
                }
            }
            self.inner.store.lock().await
        };
        let Ok(mut store) = self
            .within_admission(deadline, &operation_id, target, acquire_store)
            .await
        else {
            self.drop_reservation(&operation_id, &live, false);
            let _result = reply.send(Err(admission_expired(&operation_id, target)));
            return;
        };
        let eligible = || clock.monotonic_now() < deadline && !reply.is_closed();
        match store
            .admit_with_target(operation, target, None, eligible)
            .await
        {
            Ok(Some(ProviderOperationAdmissionResult::Admitted(record))) => {
                // Keep the same store guard until a late admission is terminal.
                // Nobody can hand it to an owner after the caller gets known unsent.
                if eligible() {
                    let result = snapshot_from_record(record)
                        .map(|snapshot| PreparedOperation::Admitted {
                            snapshot,
                            live: Arc::clone(&live),
                        })
                        .map_err(|message| snapshot_failure(operation_id.clone(), message));
                    if result.is_ok() {
                        // A queued reply can race the caller's deadline. Custody
                        // transfers only if the caller consumed the admission.
                        if reply.send(result).is_ok() && accepted.await.is_ok() {
                            live.admitted();
                            return;
                        }
                    } else {
                        let _result = reply.send(result);
                    }
                }
                if let Err(error) = store
                    .record_terminal(
                        &operation_id,
                        ProviderOperationEffect::None,
                        ProviderReconciliationState::Confirmed,
                        None,
                        self.inner.now_ms(),
                    )
                    .await
                {
                    tracing::error!(operation = operation_id.as_str(), %error, "could not settle unsent provider admission");
                }
                self.drop_reservation(&operation_id, &live, false);
            }
            Ok(Some(ProviderOperationAdmissionResult::Existing(record))) => {
                self.drop_reservation(&operation_id, &live, true);
                let _result =
                    reply.send(Self::existing_operation(operation_id, record, kind, target));
            }
            Ok(None) => {
                self.drop_reservation(&operation_id, &live, false);
                let _result = reply.send(Err(admission_expired(&operation_id, target)));
            }
            Err(error) => {
                self.drop_reservation(&operation_id, &live, false);
                tracing::warn!(operation = operation_id.as_str(), %error, "could not persist provider admission");
                let _result = reply.send(Err(admission_failure(
                    operation_id,
                    "provider operation admission could not be persisted",
                    target.cloned(),
                )));
            }
        }
    }

    /// The operation's durable row, read in one statement.
    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
    async fn stored_operation(
        &self,
        operation_id: &OperationId,
        failure_target: &Option<SessionRef>,
    ) -> Result<Option<ProviderOperationRecord>, ConversationOperationFailure> {
        self.inner
            .store
            .lock()
            .await
            .inspect(operation_id)
            .await
            .map_err(|_| {
                admission_failure(
                    operation_id.clone(),
                    "provider operation admission could not be inspected",
                    failure_target.clone(),
                )
            })
    }

    /// Waits while another request's reservation of this ID awaits its
    /// durable insert.
    pub(super) async fn wait_for_reservation(&self, operation_id: &OperationId) {
        loop {
            let reserved = self
                .inner
                .live_operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .operations
                .get(operation_id)
                .filter(|operation| operation.is_reserving())
                .cloned();
            let Some(reserved) = reserved else {
                return;
            };
            let changed = reserved.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !reserved.is_reserving() {
                return;
            }
            changed.await;
        }
    }

    /// Ends a reservation that cannot be handed to a dispatch caller.
    pub(super) fn drop_reservation(
        &self,
        operation_id: &OperationId,
        live: &Arc<LiveOperation>,
        conflict: bool,
    ) {
        let mut operations = self
            .inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if operations
            .operations
            .get(operation_id)
            .is_some_and(|held| Arc::ptr_eq(held, live))
        {
            operations.operations.remove(operation_id);
        }
        drop(operations);
        live.dropped(conflict);
    }
}

#[cfg(test)]
#[path = "provider_operation_admission_test.rs"]
mod tests;

impl super::SupervisorInner {
    pub(super) async fn release_reconciled_prompt(&self, row: &ProviderOperationRecord) {
        if row.operation_kind != ProviderOperationKind::ConversationPrompt
            || row.stage != collaboration_protocol::ProviderOperationStage::Terminal
            || row.effect != ProviderOperationEffect::Unknown
            || row.reconciliation_state
                != collaboration_protocol::ProviderReconciliationState::NotReconcilable
        {
            return;
        }
        if let ConversationBindingIdentity::ExternalProvider { binding } = &row.binding
            && let Ok(cell) = self.binding_uncertainty(binding).await
            && let Some(outcomes) = cell.outcomes.get()
        {
            crate::binding_dispatch_permit::lock(outcomes)
                .record_reconciled(row.operation_id.clone());
        }
    }

    /// The binding's uncertainty cell, seeded on first use from its durable
    /// Unknown rows (design note §14.1).
    pub(super) async fn binding_uncertainty(
        &self,
        binding: &ProviderBindingIdentity,
    ) -> Result<Arc<crate::binding_dispatch_permit::BindingUncertainty>, ()> {
        use crate::binding_dispatch_permit::{BindingOutcomes, UnknownRecovery};
        let store = &self.store;
        self.uncertainty
            .cell(&binding.endpoint, || async {
                let rows = store
                    .lock()
                    .await
                    .stored_uncertain_operations(binding)
                    .await
                    .map_err(|_| ())?;
                let mut unknown = BindingOutcomes::default();
                for row in rows {
                    use crate::session_close_evidence::{
                        RestoreCloseEvidence, RestoreUncertainty, classify_restore_uncertainty,
                    };
                    let recovery = match classify_restore_uncertainty(
                        &row,
                        RestoreCloseEvidence::ReopenedStore,
                    ) {
                        RestoreUncertainty::EarlierRestoreRetired => {
                            UnknownRecovery::RetiredRestore
                        }
                        RestoreUncertainty::EarlierCloseIncomplete => UnknownRecovery::RetiredClose,
                        RestoreUncertainty::BlocksRestore => UnknownRecovery::Blocks,
                    };
                    unknown.record_unknown(row.operation, row.target, recovery);
                }
                Ok(unknown)
            })
            .await
    }

    /// The binding's identity, for its uncertainty cell's seed.
    fn binding_identity(
        &self,
        endpoint: &collaboration_protocol::EndpointRef,
    ) -> Option<ProviderBindingIdentity> {
        self.bindings
            .get(endpoint)
            .map(|binding| binding.identity.clone())
    }

    /// The operation's live owner records its unknown outcome on its
    /// binding's uncertainty cell, before the operation stops counting as
    /// live (design note §14.1).
    pub(super) async fn record_became_unknown(
        &self,
        operation_id: &OperationId,
        live: &LiveOperation,
        target: Option<SessionRef>,
    ) {
        let Some(identity) = live
            .binding()
            .and_then(|endpoint| self.binding_identity(endpoint))
        else {
            return;
        };
        let Ok(cell) = self.binding_uncertainty(&identity).await else {
            return;
        };
        let Some(record) = cell.outcomes.get() else {
            return;
        };
        crate::binding_dispatch_permit::lock(record).record_unknown(
            operation_id.clone(),
            target.or_else(|| live.target()),
            crate::binding_dispatch_permit::UnknownRecovery::Blocks,
        );
        live.uncertainty_recorded();
    }
}
