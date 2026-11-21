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
    ProviderReconciliationState, SessionRef, admission_failure, failure,
    provider_admission_policy::{AdmissionMode, AdmissionRequest, EarlierOperation},
    provider_operation_index::Reservation,
    snapshot_failure, snapshot_from_record, unavailable_failure,
};
use crate::{binding_dispatch_permit::BindingUncertainty, provider_operation_custody::CustodyEnd};
use router_clock::ClockInstant;
use std::{future::Future, time::Duration};
use tokio::sync::oneshot;

#[derive(Default)]
struct ReservationContext {
    cell: Option<Arc<BindingUncertainty>>,
    can_recover_close: bool,
    unconfirmed_cancel: Option<String>,
}

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

    /// Seed durable uncertainty and read owner state before the atomic reservation.
    async fn reservation_context(
        &self,
        binding: &ProviderBindingIdentity,
        request: AdmissionRequest<'_>,
    ) -> Result<ReservationContext, ()> {
        if !request.checks_blockers() {
            return Ok(ReservationContext::default());
        }
        let cell = self.inner.binding_uncertainty(binding).await?;
        let can_recover_close = request.kind.is_restore()
            || request.target.is_some_and(|target| {
                self.owner_session_activity(target) != crate::ProviderSessionActivity::NotLoaded
            });
        let unconfirmed_cancel = if request.kind == ProviderOperationKind::ConversationClose {
            request
                .target
                .and_then(|target| self.owner_session_snapshot(target))
                .and_then(|snapshot| snapshot.cancel_unconfirmed)
                .map(|input| input.operation.to_string())
        } else {
            None
        };
        Ok(ReservationContext {
            cell: Some(cell),
            can_recover_close,
            unconfirmed_cancel,
        })
    }

    #[expect(
        clippy::result_large_err,
        reason = "The admission preserves its typed refusal."
    )]
    async fn reserve_operation(
        &self,
        deadline: ClockInstant,
        binding: &ProviderBindingIdentity,
        request: AdmissionRequest<'_>,
        context: &ReservationContext,
        live: &Arc<LiveOperation>,
    ) -> Result<Reservation, ConversationOperationFailure> {
        let mut changes = context
            .cell
            .as_ref()
            .map(|cell| cell.subscribe_eligibility());
        let reserved = loop {
            if let Some(changes) = &mut changes {
                changes.borrow_and_update();
            }
            let decision = {
                // Take binding outcomes before the live index, as the store lane does.
                // All store reads finish before these guards. Reconcile may take the
                // index under the store guard and releases the store before outcomes.
                let outcomes = context
                    .cell
                    .as_ref()
                    .and_then(|cell| cell.outcomes.get())
                    .map(crate::binding_dispatch_permit::lock);
                let mut operations = self
                    .inner
                    .live_operations
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if self.inner.clock.monotonic_now() >= deadline {
                    return Err(admission_expired(request.operation, request.target));
                }
                let pending = outcomes.as_ref().is_some_and(|outcomes| {
                    request.target.is_some_and(|target| {
                        outcomes.pending_dispatches_for(target).any(|operation| {
                            request.blocked_by(
                                operation,
                                EarlierOperation::InFlight {
                                    target: Some(target),
                                },
                                None,
                            )
                        })
                    })
                });
                let unknown = outcomes.as_ref().map_or_else(Vec::new, |outcomes| {
                    outcomes.blockers(request.target, context.can_recover_close)
                });
                let blocked_by_unknown = unknown.iter().any(|operation| {
                    request.blocked_by(
                        operation,
                        EarlierOperation::Unknown,
                        context.unconfirmed_cancel.as_deref(),
                    )
                });
                if pending
                    && !blocked_by_unknown
                    && !operations.operations.contains_key(request.operation)
                {
                    None
                } else {
                    Some(operations.reserve(
                        binding,
                        request,
                        live,
                        unknown,
                        context.unconfirmed_cancel.as_deref(),
                    ))
                }
            };
            if let Some(reservation) = decision {
                break reservation;
            }
            let Some(changes) = &mut changes else {
                return Err(unavailable_failure(
                    request.operation.clone(),
                    request.target.cloned(),
                ));
            };
            self.within_admission(
                deadline,
                request.operation,
                request.target,
                changes.changed(),
            )
            .await?
            .map_err(|_| unavailable_failure(request.operation.clone(), request.target.cloned()))?;
        };
        reserved.map_err(|blocking| {
            let Some(answer) = blocking.answer() else {
                return unavailable_failure(request.operation.clone(), request.target.cloned());
            };
            failure(
                ConversationOperationFailureKind::Busy,
                ConversationOperationFailureStage::Validation,
                ProviderOperationEffect::None,
                &answer,
                request.operation.clone(),
                request.target.cloned(),
            )
        })
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
        mode: AdmissionMode,
    ) -> Result<Option<Arc<LiveOperation>>, ConversationOperationFailure> {
        let kind = ProviderOperationKind::ConversationPrompt;
        let existing = self
            .within_admission(deadline, operation_id, Some(target), async {
                self.wait_for_reservation(operation_id).await;
                self.stored_operation(operation_id, &Some(target.clone()))
                    .await
            })
            .await??;
        if let Some(record) = existing {
            Self::existing_operation(operation_id.clone(), record, kind, Some(target))?;
            return Ok(None);
        }
        let request = AdmissionRequest {
            operation: operation_id,
            kind,
            target: Some(target),
            mode,
        };
        let context = self
            .within_admission(
                deadline,
                operation_id,
                Some(target),
                self.reservation_context(binding, request),
            )
            .await?
            .map_err(|()| unavailable_failure(operation_id.clone(), Some(target.clone())))?;
        let live = Arc::new(LiveOperation::queued_prompt(binding, target.clone()));
        match self
            .reserve_operation(deadline, binding, request, &context, &live)
            .await?
        {
            Reservation::Acquired => Ok(Some(live)),
            Reservation::Occupied(prior) if prior.is_prompt_for(target) => Ok(None),
            Reservation::Occupied(_) => Err(reused_id_failure(
                operation_id.clone(),
                Some(target.clone()),
            )),
        }
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
        mode: AdmissionMode,
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
        let request = AdmissionRequest {
            operation: &operation_id,
            kind: operation_kind,
            target: known_target,
            mode,
        };
        let context = self
            .within_admission(
                deadline,
                &operation_id,
                known_target,
                self.reservation_context(&binding, request),
            )
            .await?
            .map_err(|()| unavailable_failure(operation_id.clone(), failure_target.clone()))?;

        #[cfg(test)]
        self.pause_before_admission_reservation_for_test().await;
        let live = Arc::new(
            LiveOperation::reserving(&binding, known_target.cloned(), operation_kind, mode.mark)
                .with_cancel_target(mode.cancel_target),
        );
        if matches!(
            self.reserve_operation(deadline, &binding, request, &context, &live)
                .await?,
            Reservation::Occupied(_)
        ) {
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
        live.host_write_begun();
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
                let end = match store
                    .record_terminal(
                        &operation_id,
                        ProviderOperationEffect::None,
                        ProviderReconciliationState::Confirmed,
                        None,
                        self.inner.now_ms(),
                    )
                    .await
                {
                    Ok(row) => {
                        self.inner.refresh_durable_operation(&row);
                        CustodyEnd::Written
                    }
                    Err(error) => {
                        tracing::error!(operation = operation_id.as_str(), %error, "could not settle unsent provider admission");
                        CustodyEnd::Failed
                    }
                };
                self.drop_reservation_after(&operation_id, &live, false, end);
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
                self.drop_reservation_after(&operation_id, &live, false, CustodyEnd::Failed);
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
        self.drop_reservation_after(operation_id, live, conflict, CustodyEnd::Dropped);
    }

    fn drop_reservation_after(
        &self,
        operation_id: &OperationId,
        live: &Arc<LiveOperation>,
        conflict: bool,
        end: CustodyEnd,
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
        live.dropped(conflict, end);
        self.inner.wake_operation_eligibility(live);
    }
}

#[cfg(test)]
#[path = "provider_operation_admission_test.rs"]
mod tests;

impl super::SupervisorInner {
    pub(super) fn refresh_durable_operation(&self, row: &ProviderOperationRecord) {
        if let ConversationBindingIdentity::ExternalProvider { binding } = &row.binding {
            self.uncertainty
                .slot(&binding.endpoint)
                .refresh_durable_row(row, None);
        }
    }

    /// The binding's uncertainty cell, seeded on first use from its durable
    /// Unknown rows (design note §14.1).
    pub(super) async fn binding_uncertainty(
        &self,
        binding: &ProviderBindingIdentity,
    ) -> Result<Arc<crate::binding_dispatch_permit::BindingUncertainty>, ()> {
        use crate::binding_dispatch_permit::{BindingOutcomes, UnknownRecovery};
        let cell = self.uncertainty.slot(&binding.endpoint);
        if cell.outcomes.get().is_none() {
            let mut store = self.store.lock().await;
            if cell.outcomes.get().is_none() {
                let rows = store
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
                let _ = cell.outcomes.set(std::sync::Mutex::new(unknown));
            }
        }
        Ok(cell)
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

    /// Record the unknown outcome before releasing row custody. Later
    /// admissions need to see either the live writer or the uncertainty.
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
