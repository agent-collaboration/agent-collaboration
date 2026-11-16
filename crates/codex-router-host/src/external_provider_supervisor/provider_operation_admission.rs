//! One atomic admission per operation ID, under the operation store's mutex:
//! a reused ID must match its request, an unresolved earlier outcome blocks
//! the session unless it is a live turn, and a new row is persisted before
//! anything reaches the agent (D1 Change 4 and Change 5).

use super::{
    Arc, ConversationBindingIdentity, ConversationOperationFailure,
    ConversationOperationFailureKind, ConversationOperationFailureStage,
    ConversationOperationSnapshot, ExternalProviderSupervisor, LiveOperation, OperationId,
    PreparedOperation, ProviderBindingIdentity, ProviderOperationAdmission,
    ProviderOperationAdmissionResult, ProviderOperationEffect, ProviderOperationKind,
    ProviderOperationRecord, ProviderOperationStore, SessionRef, admission_failure, failure,
    now_ms, snapshot_failure, snapshot_from_record, unavailable_failure,
};
use std::sync::atomic::Ordering;

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
fn same_request(
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
            return Err(failure(
                ConversationOperationFailureKind::InvalidRequest,
                ConversationOperationFailureStage::Validation,
                ProviderOperationEffect::None,
                REUSED_ID_MESSAGE,
                operation_id,
                known_target.cloned(),
            ));
        }
        Ok(PreparedOperation::Existing(
            snapshot_from_record(record)
                .map_err(|message| snapshot_failure(operation_id, message))?,
        ))
    }

    /// The unresolved operations that still block this admission. For an API
    /// prompt, a row that is a live turn (MayHaveDispatched, its result not
    /// yet published) lets the request through to the busy decision; a
    /// terminal Unknown row always blocks (C2).
    pub(super) async fn still_blocking(
        &self,
        store: &mut ProviderOperationStore,
        blocking: Vec<OperationId>,
        mark: DispatchMark,
    ) -> Result<Vec<OperationId>, ()> {
        if mark == DispatchMark::AtAdmission {
            return Ok(blocking);
        }
        let mut still = Vec::new();
        for operation_id in blocking {
            let live_turn = match store.inspect(&operation_id).await.map_err(|_| ())? {
                Some(record) => {
                    record.stage
                        == collaboration_protocol::ProviderOperationStage::MayHaveDispatched
                        && self
                            .inner
                            .live_operations
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .operations
                            .get(&operation_id)
                            .is_some_and(|live| !live.published.load(Ordering::SeqCst))
                }
                None => false,
            };
            if !live_turn {
                still.push(operation_id);
            }
        }
        Ok(still)
    }

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
        let mut store = self.inner.store.lock().await;
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
        if let Some(record) = store.inspect(&operation_id).await.map_err(|_| {
            admission_failure(
                operation_id.clone(),
                "provider operation admission could not be inspected",
                failure_target.clone(),
            )
        })? {
            return Self::existing_operation(operation_id, record, operation_kind, known_target);
        }
        if operation_kind != ProviderOperationKind::ConversationCancel {
            let blocking = store
                .blocking_uncertain_operation_ids(&binding, known_target)
                .await
                .map_err(|_| unavailable_failure(operation_id.clone(), failure_target.clone()))?;
            let blocking = self
                .still_blocking(&mut store, blocking, mark)
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
        let admission = store
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
            ProviderOperationAdmissionResult::Existing(record) => {
                return Self::existing_operation(
                    operation_id,
                    record,
                    operation_kind,
                    known_target,
                );
            }
            ProviderOperationAdmissionResult::Admitted(record) => record,
        };
        let record = match known_target {
            Some(target) => store
                .record_target(&operation_id, target, now_ms())
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
            DispatchMark::AtAdmission => store
                .mark_may_have_dispatched(&operation_id, now_ms())
                .await
                .map_err(|_| {
                    admission_failure(
                        operation_id.clone(),
                        "provider dispatch marker could not be persisted",
                        failure_target,
                    )
                })?,
            DispatchMark::Deferred => record,
        };
        let snapshot = snapshot_from_record(record)
            .map_err(|message| snapshot_failure(operation_id.clone(), message))?;
        let live = Arc::new(LiveOperation::pending());
        self.inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .operations
            .insert(operation_id, Arc::clone(&live));
        Ok(PreparedOperation::Admitted { snapshot, live })
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

#[cfg(test)]
#[path = "provider_operation_admission_test.rs"]
mod tests;
