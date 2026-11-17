//! One admission per operation ID (D1 Change 4 and Change 5; state machines,
//! design note §14.1–§15.3): a reused ID must match its request, an earlier
//! send's unknown outcome blocks the session, and a new row is persisted
//! before anything reaches the agent. Decisions read the binding's cells;
//! the store lock covers single statements only.

use super::{
    Arc, ConversationBindingIdentity, ConversationOperationFailure,
    ConversationOperationFailureKind, ConversationOperationFailureStage,
    ConversationOperationSnapshot, ExternalProviderSupervisor, LiveOperation, OperationId,
    PreparedOperation, ProviderBindingIdentity, ProviderOperationAdmission,
    ProviderOperationAdmissionResult, ProviderOperationEffect, ProviderOperationKind,
    ProviderOperationRecord, SessionRef, admission_failure, failure, now_ms, snapshot_failure,
    snapshot_from_record, unavailable_failure,
};
/// When an admitted operation is marked as possibly dispatched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DispatchMark {
    /// At admission: the caller dispatches right away.
    AtAdmission,
    /// Later, by `begin_dispatch`, once the API prompt path knows it will
    /// prompt or steer; a queued API prompt stays Admitted with no effect.
    Deferred,
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
    ) -> Result<Vec<OperationId>, ()> {
        let cell = self.inner.binding_uncertainty(binding).await?;
        let mut blocking = match super::provider_operation_index::apply_uncertainty(
            &cell,
            crate::uncertainty_cell_machine::UncertaintyEvent::AdmissionCheck {
                target: known_target.cloned(),
            },
        ) {
            Ok(()) => Vec::new(),
            Err(crate::uncertainty_cell_machine::UncertaintyRefusal::EarlierOutcomeUnknown {
                operations,
            }) => operations
                .into_iter()
                .filter_map(|operation| OperationId::try_from(operation).ok())
                .collect(),
        };
        if mark == DispatchMark::AtAdmission {
            blocking.extend(
                self.inner
                    .live_operations
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .in_flight_on(&binding.endpoint, known_target),
            );
        }
        Ok(blocking)
    }

    /// Admits one operation (design note §15.3). A known ID answers from its
    /// row; otherwise the binding's cells decide whether an earlier outcome
    /// blocks it, the operation is reserved in the live index before its
    /// durable insert, and each durable step is one statement. No store lock
    /// is held across a decision.
    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
    pub(super) async fn admit_operation(
        &self,
        operation_id: OperationId,
        operation_kind: ProviderOperationKind,
        binding: ProviderBindingIdentity,
        known_target: Option<&SessionRef>,
        mark: DispatchMark,
    ) -> Result<PreparedOperation, ConversationOperationFailure> {
        let admitted_at_ms = now_ms();
        let failure_target = known_target.cloned();
        // 1. A reserved ID: wait for its durable insert, then answer from it.
        self.wait_for_reservation(&operation_id).await;
        if let Some(record) = self
            .stored_operation(&operation_id, &failure_target)
            .await?
        {
            return Self::existing_operation(operation_id, record, operation_kind, known_target);
        }
        // 2. Whether an earlier outcome blocks it, from the binding's cells.
        if operation_kind != ProviderOperationKind::ConversationCancel {
            let blocking = self
                .admission_blockers(&binding, known_target, mark)
                .await
                .map_err(|()| unavailable_failure(operation_id.clone(), failure_target.clone()))?;
            if !blocking.is_empty() {
                let ids = blocking
                    .iter()
                    .map(OperationId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(failure(
                    ConversationOperationFailureKind::Busy,
                    ConversationOperationFailureStage::Validation,
                    ProviderOperationEffect::None,
                    &format!("provider binding has unresolved operations: {ids}"),
                    operation_id,
                    failure_target,
                ));
            }
        }
        // 3. Reserve it in the live index before its durable insert.
        let live = Arc::new(LiveOperation::reserving(
            binding.endpoint.clone(),
            known_target.cloned(),
        ));
        let reserved_first = {
            let mut operations = self
                .inner
                .live_operations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if operations.operations.contains_key(&operation_id) {
                true
            } else {
                operations
                    .operations
                    .insert(operation_id.clone(), Arc::clone(&live));
                false
            }
        };
        if reserved_first {
            // Another request reserved this ID first: answer from it.
            self.wait_for_reservation(&operation_id).await;
            return match self
                .stored_operation(&operation_id, &failure_target)
                .await?
            {
                Some(record) => {
                    Self::existing_operation(operation_id, record, operation_kind, known_target)
                }
                None => Err(unavailable_failure(operation_id, failure_target)),
            };
        }
        // A test holds the reservation here, before its durable insert.
        #[cfg(test)]
        let admission_test_pause = {
            self.inner
                .admission_test_pause
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
        };
        #[cfg(test)]
        if let Some(pause) = admission_test_pause {
            let _result = pause.entered.send(());
            let _result = pause.release.await;
        }
        // 4. The durable steps, one statement each.
        match self
            .persist_admission(
                &operation_id,
                operation_kind,
                binding,
                known_target,
                mark,
                admitted_at_ms,
                &live,
            )
            .await
        {
            Ok(Some(snapshot)) => {
                live.admitted();
                Ok(PreparedOperation::Admitted { snapshot, live })
            }
            Ok(None) => {
                // A durable row already existed: this reservation ends.
                self.drop_reservation(&operation_id, &live, true);
                match self
                    .stored_operation(&operation_id, &failure_target)
                    .await?
                {
                    Some(record) => {
                        Self::existing_operation(operation_id, record, operation_kind, known_target)
                    }
                    None => Err(unavailable_failure(operation_id, failure_target)),
                }
            }
            Err(admission_failure) => {
                self.drop_reservation(&operation_id, &live, false);
                Err(admission_failure)
            }
        }
    }

    /// The insert, its target and, at admission, its dispatch marker; `None`
    /// when a durable row for the ID already existed.
    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
    #[expect(
        clippy::too_many_arguments,
        reason = "One admission's durable inputs, passed through from its caller."
    )]
    async fn persist_admission(
        &self,
        operation_id: &OperationId,
        operation_kind: ProviderOperationKind,
        binding: ProviderBindingIdentity,
        known_target: Option<&SessionRef>,
        mark: DispatchMark,
        admitted_at_ms: i64,
        live: &LiveOperation,
    ) -> Result<Option<ConversationOperationSnapshot>, ConversationOperationFailure> {
        let failure_target = known_target.cloned();
        let admission = self
            .inner
            .store
            .lock()
            .await
            .admit(ProviderOperationAdmission {
                operation_id: operation_id.clone(),
                operation_kind,
                binding: ConversationBindingIdentity::ExternalProvider { binding },
                admitted_at_ms,
            })
            .await
            .map_err(|_| {
                admission_failure(
                    operation_id.clone(),
                    "provider operation admission could not be persisted",
                    failure_target.clone(),
                )
            })?;
        let record = match admission {
            ProviderOperationAdmissionResult::Existing(_) => return Ok(None),
            ProviderOperationAdmissionResult::Admitted(record) => record,
        };
        let record = match known_target {
            Some(target) => self
                .inner
                .store
                .lock()
                .await
                .record_target(operation_id, target, now_ms())
                .await
                .map_err(|_| {
                    admission_failure(
                        operation_id.clone(),
                        "provider operation target could not be persisted",
                        failure_target.clone(),
                    )
                })?,
            None => record,
        };
        let record = match mark {
            DispatchMark::AtAdmission => {
                live.dispatch_marked();
                self.inner
                    .store
                    .lock()
                    .await
                    .mark_may_have_dispatched(operation_id, now_ms())
                    .await
                    .map_err(|_| {
                        admission_failure(
                            operation_id.clone(),
                            "provider dispatch marker could not be persisted",
                            failure_target,
                        )
                    })?
            }
            DispatchMark::Deferred => record,
        };
        snapshot_from_record(record)
            .map(Some)
            .map_err(|message| snapshot_failure(operation_id.clone(), message))
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

    /// Ends a reservation that got no row of its own: a durable row already
    /// existed (`conflict`), or the insert failed.
    fn drop_reservation(
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

    /// Marks a Deferred admission as possibly dispatched, right before its
    /// prompt or steer is sent.
    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
    pub(super) async fn begin_dispatch(
        &self,
        operation_id: &OperationId,
        target: &SessionRef,
    ) -> Result<ConversationOperationSnapshot, ConversationOperationFailure> {
        self.with_live(operation_id, LiveOperation::dispatch_marked);
        let record = self
            .inner
            .store
            .lock()
            .await
            .mark_may_have_dispatched(operation_id, now_ms())
            .await
            .map_err(|_| {
                admission_failure(
                    operation_id.clone(),
                    "provider dispatch marker could not be persisted",
                    Some(target.clone()),
                )
            })?;
        snapshot_from_record(record)
            .map_err(|message| snapshot_failure(operation_id.clone(), message))
    }
}

impl ExternalProviderSupervisor {
    /// Undoes `begin_dispatch` when nothing was sent: the row is waiting
    /// again, so `show` reports its line state and a restart finds it
    /// admitted, never sent (D1 R14).
    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
    pub(super) async fn undo_dispatch_mark(
        &self,
        operation_id: &OperationId,
        target: &SessionRef,
    ) -> Result<(), ConversationOperationFailure> {
        let undone = self
            .inner
            .store
            .lock()
            .await
            .unmark_undispatched(operation_id, now_ms())
            .await
            .map(|_| ());
        if undone.is_ok() {
            self.with_live(operation_id, LiveOperation::undispatched);
        }
        undone.map_err(|_| {
            admission_failure(
                operation_id.clone(),
                "provider dispatch marker could not be undone",
                Some(target.clone()),
            )
        })
    }

    /// Applies `step` to the operation's live owner, when it has one.
    fn with_live(&self, operation_id: &OperationId, step: impl FnOnce(&LiveOperation)) {
        let live = self
            .inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .operations
            .get(operation_id)
            .cloned();
        if let Some(live) = live {
            step(&live);
        }
    }
}

#[cfg(test)]
#[path = "provider_operation_admission_test.rs"]
mod tests;

impl super::SupervisorInner {
    /// The binding's uncertainty cell, seeded on first use from its durable
    /// Unknown rows (design note §14.1).
    pub(super) async fn binding_uncertainty(
        &self,
        binding: &ProviderBindingIdentity,
    ) -> Result<
        Arc<
            tokio::sync::OnceCell<
                std::sync::Mutex<crate::uncertainty_cell_machine::UnknownOutcomes>,
            >,
        >,
        (),
    > {
        let store = &self.store;
        self.uncertainty
            .cell(&binding.endpoint, || async {
                // Every Unknown row of this binding's generation blocks, as the
                // durable scan did, reconciled or not.
                store
                    .lock()
                    .await
                    .unknown_operations(binding)
                    .await
                    .map_err(|_| ())
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
        let _recorded = super::provider_operation_index::apply_uncertainty(
            &cell,
            crate::uncertainty_cell_machine::UncertaintyEvent::BecameUnknown {
                operation: operation_id.clone(),
                target: target.or_else(|| live.target()),
            },
        );
        live.uncertainty_recorded();
    }
}
