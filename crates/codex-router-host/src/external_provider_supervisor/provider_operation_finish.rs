//! An operation's one completion owner writes its outcome to the store and
//! publishes it (design note §11.5, §12.5): the record takes the outcome
//! first, an unknown outcome reaches the binding's uncertainty cell before
//! the operation stops counting as live, and then the durable row is
//! settled.

use super::{LiveOperation, SupervisorInner, failure, now_ms};
use crate::provider_operation_settlement::ProviderOperationCompletion;
use collaboration_protocol::{
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ConversationOperationSettlement, OperationId,
    ProviderOperationEffect, ProviderPromptStopReason, ProviderReconciliationState, SessionRef,
};
use std::sync::Arc;

impl SupervisorInner {
    pub(super) async fn finish(
        &self,
        operation_id: OperationId,
        live: Arc<LiveOperation>,
        completion: ProviderOperationCompletion,
    ) {
        // A test holds this operation here, before its terminal write.
        #[cfg(test)]
        let terminal_write_test_pause = {
            let mut pause = self
                .terminal_write_test_pause
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match pause.take() {
                Some((held, pause)) if held == operation_id => Some(pause),
                other => {
                    *pause = other;
                    None
                }
            }
        };
        #[cfg(test)]
        if let Some(pause) = terminal_write_test_pause {
            let _result = pause.entered.send(());
            let _result = pause.release.await;
        }
        // The operation's record takes its outcome first; an unknown one
        // stays live until the binding's uncertainty cell holds it.
        live.completed(match &completion {
            ProviderOperationCompletion::Success { .. }
            | ProviderOperationCompletion::FailureWithSession { .. } => {
                crate::operation_record_machine::AttemptEnding::Applied
            }
            ProviderOperationCompletion::Failure(failure) => match failure.effect {
                ProviderOperationEffect::Applied => {
                    crate::operation_record_machine::AttemptEnding::Applied
                }
                ProviderOperationEffect::None => {
                    crate::operation_record_machine::AttemptEnding::Refused
                }
                ProviderOperationEffect::Unknown => {
                    crate::operation_record_machine::AttemptEnding::Unknown
                }
            },
        });
        let result = match completion {
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
                    Err(applied_storage_failure(operation_id.clone(), target))
                } else {
                    Ok(settlement)
                }
            }
            ProviderOperationCompletion::Failure(operation_failure) => {
                let reconciliation = if operation_failure.effect == ProviderOperationEffect::Unknown
                {
                    // Recorded on the binding's uncertainty cell before this
                    // operation stops counting as live (design note §14.1).
                    self.record_became_unknown(
                        &operation_id,
                        &live,
                        operation_failure.target.clone(),
                    )
                    .await;
                    ProviderReconciliationState::Unresolved
                } else {
                    ProviderReconciliationState::Confirmed
                };
                let mut store = self.store.lock().await;
                let target = operation_failure.target.clone();
                let target_record_failed =
                    if operation_failure.effect == ProviderOperationEffect::Applied {
                        match target.as_ref() {
                            Some(target) => store
                                .record_target(&operation_id, target, now_ms())
                                .await
                                .is_err(),
                            None => false,
                        }
                    } else {
                        false
                    };
                if target_record_failed {
                    Err(applied_storage_failure(operation_id.clone(), target))
                } else {
                    let _ = store
                        .record_terminal(
                            &operation_id,
                            operation_failure.effect,
                            reconciliation,
                            None,
                            now_ms(),
                        )
                        .await;
                    Err(operation_failure)
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
                    Err(applied_storage_failure(
                        operation_id.clone(),
                        Some(session_record.target.clone()),
                    ))
                } else {
                    Err(failure)
                }
            }
        };
        self.publish_result(operation_id, &live, result).await;
    }
}

fn applied_storage_failure(
    operation_id: OperationId,
    target: Option<SessionRef>,
) -> ConversationOperationFailure {
    failure(
        ConversationOperationFailureKind::Unavailable,
        ConversationOperationFailureStage::Settlement,
        ProviderOperationEffect::Applied,
        "provider settlement could not be persisted after the operation applied",
        operation_id,
        target,
    )
}
