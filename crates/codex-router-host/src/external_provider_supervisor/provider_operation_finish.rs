//! An operation's one completion owner writes its outcome to the store and
//! publishes it (design note §11.5, §12.5): the record takes the outcome
//! first, an unknown outcome reaches the binding's uncertainty cell before
//! the operation stops counting as live, and then the durable row is
//! settled.

use super::{LiveOperation, SupervisorInner, failure};
use crate::{
    provider_operation_settlement::ProviderOperationCompletion, session_store_lane::STORE_WAIT,
};
use collaboration_protocol::{
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ConversationOperationSettlement, OperationId,
    ProviderOperationEffect, ProviderOperationStage, ProviderPromptStopReason,
    ProviderReconciliationState, SessionRef,
};
use std::sync::Arc;

impl SupervisorInner {
    pub(super) async fn settle_late_dispatch_mark(
        &self,
        store: &mut collaboration_runtime::ProviderOperationStore,
        operation: &OperationId,
        failure: &ConversationOperationFailure,
    ) {
        if let Err(error) = self
            .write_terminal_failure(
                store,
                operation,
                failure,
                ProviderReconciliationState::Confirmed,
            )
            .await
        {
            tracing::error!(operation = operation.as_str(), %error, "could not settle the late mark of an unsent operation");
        }
    }

    async fn write_terminal_failure(
        &self,
        store: &mut collaboration_runtime::ProviderOperationStore,
        operation: &OperationId,
        failure: &ConversationOperationFailure,
        reconciliation: ProviderReconciliationState,
    ) -> Result<
        collaboration_runtime::ProviderOperationRecord,
        collaboration_runtime::ProviderOperationStoreError,
    > {
        store
            .record_terminal(
                operation,
                failure.effect,
                reconciliation,
                None,
                self.now_ms(),
            )
            .await
    }

    pub(super) async fn finish(
        &self,
        operation_id: OperationId,
        live: Arc<LiveOperation>,
        completion: ProviderOperationCompletion,
    ) {
        // The operation's record takes its outcome first; an unknown one
        // stays live until the binding's uncertainty cell holds it.
        live.completed(match &completion {
            ProviderOperationCompletion::Success { .. }
            | ProviderOperationCompletion::FailureWithSession { .. } => {
                provider_session_machines::operation_record_machine::AttemptEnding::Applied
            }
            ProviderOperationCompletion::Failure(failure) => match failure.effect {
                ProviderOperationEffect::Applied => {
                    provider_session_machines::operation_record_machine::AttemptEnding::Applied
                }
                ProviderOperationEffect::None => {
                    provider_session_machines::operation_record_machine::AttemptEnding::Refused
                }
                ProviderOperationEffect::Unknown => {
                    provider_session_machines::operation_record_machine::AttemptEnding::Unknown
                }
            },
            ProviderOperationCompletion::DispatchMarkNotWritten(_) => {
                provider_session_machines::operation_record_machine::AttemptEnding::Refused
            }
            ProviderOperationCompletion::Owned { result, .. } => {
                match result.as_ref().err().map(|failure| failure.effect) {
                    None | Some(ProviderOperationEffect::Applied) => {
                        provider_session_machines::operation_record_machine::AttemptEnding::Applied
                    }
                    Some(ProviderOperationEffect::None) => {
                        provider_session_machines::operation_record_machine::AttemptEnding::Refused
                    }
                    Some(ProviderOperationEffect::Unknown) => {
                        provider_session_machines::operation_record_machine::AttemptEnding::Unknown
                    }
                }
            }
        });
        let (result, retention) = match completion {
            ProviderOperationCompletion::DispatchMarkNotWritten(failure) => {
                (Err(failure), Retention::PersistenceFailed)
            }
            ProviderOperationCompletion::Success {
                settlement,
                target,
                session_record,
            } => {
                let terminal_stop_reason = match &settlement {
                    // The durable summary column is an old closed string enum.
                    // Keep a future value in the live settlement without writing
                    // an unreadable value into rollback-visible storage.
                    ConversationOperationSettlement::PromptCompleted {
                        stop_reason: ProviderPromptStopReason::Unknown(_),
                        ..
                    } => None,
                    ConversationOperationSettlement::PromptCompleted { stop_reason, .. } => {
                        Some(stop_reason.clone())
                    }
                    _ => None,
                };
                let mut store = self.store.lock().await;
                if crate::provider_operation_settlement::persist_provider_success(
                    &mut store,
                    &operation_id,
                    target.as_ref(),
                    session_record.as_deref(),
                    terminal_stop_reason,
                )
                .await
                .is_err()
                {
                    let durable = durable_stage(&mut store, &operation_id).await;
                    (
                        Err(applied_unsaved(operation_id.clone(), target, durable)),
                        Retention::PersistenceFailed,
                    )
                } else {
                    (Ok(settlement), Retention::Settled)
                }
            }
            ProviderOperationCompletion::Failure(operation_failure) => {
                let restore_failure = live.is_restore().then(|| operation_failure.clone());
                let reconciliation = if operation_failure.effect == ProviderOperationEffect::Unknown
                {
                    // Recorded on the binding's uncertainty cell before this
                    // operation stops counting as live (design note §14.1).
                    if live.is_restore() {
                        // The local restore has already retired. Its unknown
                        // provider effect does not bar deliberate recovery.
                        live.uncertainty_recorded();
                    } else {
                        self.record_became_unknown(
                            &operation_id,
                            &live,
                            operation_failure.target.clone(),
                        )
                        .await;
                    }
                    ProviderReconciliationState::Unresolved
                } else {
                    ProviderReconciliationState::Confirmed
                };
                let persistence =
                    self.persist_failed_operation(&operation_id, operation_failure, reconciliation);
                if let Some(restore_failure) = restore_failure {
                    tokio::pin!(persistence);
                    let bounded = tokio::select! {
                        biased;
                        () = self.operation_cleanup_cancel.cancelled() => Err(router_clock::WaitElapsed),
                        written = router_clock::timeout(self.clock.as_ref(), STORE_WAIT, &mut persistence) => written,
                    };
                    match bounded {
                        Ok(settled) => settled,
                        Err(_) => {
                            self.publish_result(
                                operation_id.clone(),
                                &live,
                                Err(restore_failure),
                                Retention::PersistenceFailed,
                            )
                            .await;
                            // Keep the write in the same shutdown-tracked task.
                            // Late persistence cannot replace the delivered failure.
                            tokio::select! {
                                biased;
                                () = self.operation_cleanup_cancel.cancelled() => {}
                                (_, retention) = &mut persistence => {
                                    if retention == Retention::PersistenceFailed {
                                        tracing::error!(operation = operation_id.as_str(), "could not settle the late terminal write for a restore failure");
                                    }
                                }
                            }
                            return;
                        }
                    }
                } else {
                    persistence.await
                }
            }
            ProviderOperationCompletion::FailureWithSession {
                failure,
                session_record,
            } => {
                let mut store = self.store.lock().await;
                if store
                    .settle_session_operation(&operation_id, &session_record)
                    .await
                    .is_err()
                {
                    let durable = durable_stage(&mut store, &operation_id).await;
                    (
                        Err(applied_unsaved(
                            operation_id.clone(),
                            Some(session_record.target.clone()),
                            durable,
                        )),
                        Retention::PersistenceFailed,
                    )
                } else {
                    (Err(failure), Retention::Settled)
                }
            }
            ProviderOperationCompletion::Owned {
                result,
                target,
                saved,
            } => {
                if result
                    .as_ref()
                    .is_err_and(|failure| failure.effect == ProviderOperationEffect::Unknown)
                {
                    live.uncertainty_recorded();
                }
                if saved {
                    (result, Retention::Settled)
                } else {
                    let unsaved = match result {
                        Ok(_) => failure(
                            ConversationOperationFailureKind::Unavailable,
                            ConversationOperationFailureStage::Settlement,
                            ProviderOperationEffect::Applied,
                            "the provider effect was applied; the saved result isn't available and may differ",
                            operation_id.clone(),
                            Some(target),
                        ),
                        Err(mut observed) => {
                            let summary = format!(
                                "{}; the saved result isn't available and may differ",
                                String::from(observed.message.clone())
                            );
                            if let Ok(message) =
                                collaboration_protocol::NonEmptyText::try_from(summary)
                            {
                                observed.message = message;
                            }
                            observed
                        }
                    };
                    (Err(unsaved), Retention::PersistenceFailed)
                }
            }
        };
        self.publish_result(operation_id, &live, result, retention)
            .await;
    }

    async fn persist_failed_operation(
        &self,
        operation_id: &OperationId,
        operation_failure: ConversationOperationFailure,
        reconciliation: ProviderReconciliationState,
    ) -> (
        Result<ConversationOperationSettlement, ConversationOperationFailure>,
        Retention,
    ) {
        let mut store = self.store.lock().await;
        let target = operation_failure.target.clone();
        let target_record_failed = if operation_failure.effect == ProviderOperationEffect::Applied {
            match target.as_ref() {
                Some(target) => store
                    .record_target(operation_id, target, self.now_ms())
                    .await
                    .is_err(),
                None => false,
            }
        } else {
            false
        };
        if target_record_failed {
            let durable = durable_stage(&mut store, operation_id).await;
            (
                Err(applied_unsaved(operation_id.clone(), target, durable)),
                Retention::PersistenceFailed,
            )
        } else if self
            .write_terminal_failure(&mut store, operation_id, &operation_failure, reconciliation)
            .await
            .is_err()
        {
            let durable = durable_stage(&mut store, operation_id).await;
            (
                Err(failure_unsaved(operation_failure, durable)),
                Retention::PersistenceFailed,
            )
        } else {
            (Err(operation_failure), Retention::Settled)
        }
    }
}

/// The durable stage an operation's row still has after a failed write;
/// none when the row can't be read either.
async fn durable_stage(
    store: &mut collaboration_runtime::ProviderOperationStore,
    operation_id: &OperationId,
) -> Option<ProviderOperationStage> {
    store
        .inspect(operation_id)
        .await
        .ok()
        .flatten()
        .map(|record| record.stage)
}

/// The one line for an outcome whose durable write failed (§12.5): what
/// was observed, then the durable stage that remains. Never a blanket
/// "applied".
fn unsaved_line(observed: &str, durable: Option<ProviderOperationStage>) -> String {
    let durable = match durable {
        Some(ProviderOperationStage::Admitted) => "admitted",
        Some(ProviderOperationStage::MayHaveDispatched) => "may-have-dispatched",
        Some(ProviderOperationStage::Terminal) => "settled",
        None => "unreadable",
    };
    format!("{observed}; the durable record still says {durable} (write failed)")
}

/// An operation that applied, whose settlement didn't save.
fn applied_unsaved(
    operation_id: OperationId,
    target: Option<SessionRef>,
    durable: Option<ProviderOperationStage>,
) -> ConversationOperationFailure {
    failure(
        ConversationOperationFailureKind::Unavailable,
        ConversationOperationFailureStage::Settlement,
        ProviderOperationEffect::Applied,
        &unsaved_line("applied", durable),
        operation_id,
        target,
    )
}

/// A failed operation whose terminal write didn't save: its kind and
/// effect stay, and its message says what was observed and what the
/// durable record still says.
fn failure_unsaved(
    mut operation_failure: ConversationOperationFailure,
    durable: Option<ProviderOperationStage>,
) -> ConversationOperationFailure {
    let observed = match operation_failure.effect {
        ProviderOperationEffect::Applied => "applied",
        ProviderOperationEffect::None => "not sent",
        ProviderOperationEffect::Unknown => "outcome unknown",
    };
    let line = unsaved_line(
        &format!(
            "{observed} ({})",
            String::from(operation_failure.message.clone())
        ),
        durable,
    );
    if let Ok(message) = collaboration_protocol::NonEmptyText::try_from(line) {
        operation_failure.message = message;
    }
    operation_failure
}

/// Where a published result is retained: settled results share one bounded
/// window; results whose durable write failed keep their own, so later
/// settlements never evict them before the Router stops (§12.5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Retention {
    Settled,
    PersistenceFailed,
}
