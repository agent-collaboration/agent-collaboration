//! Test arrangement for old local state retained until handover; absent from production builds.
use super::{WakeFailureContext, WakeOperations, wake_failure};
use automation_storage::{StorageError, WakeAction, WakeCreate, WakeMutation};
use collaboration_protocol::{
    LocalMutationState, SavedMessage, WakeFailure, WakeFailureReason, WakeMutationRequest,
    WakeMutationResult, WakeSendRequest, WakeSnapshot,
};

impl WakeOperations {
    /// Creates a wake, or replays the wake an earlier request with this operation created.
    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    pub(crate) async fn wake_send_local(
        &self,
        request: WakeSendRequest,
    ) -> Result<WakeSnapshot, WakeFailure> {
        let context = WakeFailureContext::operation(request.operation_id.clone());
        let store = self.store(&context)?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let timing = serde_json::to_value(request.timing).and_then(serde_json::from_value);
        let expiry = serde_json::to_value(request.expiry).and_then(serde_json::from_value);
        let (Ok(timing), Ok(expiry)) = (timing, expiry) else {
            return Err(wake_failure(
                context,
                WakeFailureReason::InvalidField {
                    field: "timing".into(),
                    constraint: "Use a valid UTC instant or bounded duration.".into(),
                },
                LocalMutationState::None,
            ));
        };
        let created = store
            .lock()
            .await
            .create_wakeup(&WakeCreate {
                operation_id: request.operation_id,
                requested_by: request.requested_by,
                replace: request.replace,
                message: request.message,
                timing,
                expiry,
                now_ms,
            })
            .await;
        match created {
            // Creation and replay return the cursor time of the durable creation receipt.
            Ok(record) => {
                let observed = record.definition.created_at_ms;
                crate::wakeup_projection::snapshot(record, &self.service_id, observed).map_err(
                    |()| {
                        wake_failure(
                            context,
                            WakeFailureReason::InvalidRecord,
                            LocalMutationState::Committed,
                        )
                    },
                )
            }
            Err(error) => {
                let mutation = if matches!(error, StorageError::Database(_)) {
                    LocalMutationState::Unknown
                } else {
                    LocalMutationState::None
                };
                Err(wake_failure(context, storage_reason(error), mutation))
            }
        }
    }

    /// Pauses a wake; deliveries already handed to a native thread are not recalled.
    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    pub(crate) async fn wake_pause_local(
        &self,
        request: WakeMutationRequest,
    ) -> Result<WakeMutationResult, WakeFailure> {
        self.mutate(request, WakeAction::Pause).await
    }

    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    async fn mutate(
        &self,
        request: WakeMutationRequest,
        action: WakeAction,
    ) -> Result<WakeMutationResult, WakeFailure> {
        let context = WakeFailureContext {
            operation_id: Some(request.operation_id.clone()),
            wakeup_id: Some(request.wakeup_id.clone()),
        };
        let store = self.store(&context)?;
        let now = chrono::Utc::now().timestamp_millis();
        let mutated = store
            .lock()
            .await
            .mutate_wakeup::<SavedMessage>(&WakeMutation {
                operation_id: request.operation_id,
                wakeup_id: request.wakeup_id,
                action,
                now_ms: now,
            })
            .await;
        self.project_local_mutation(mutated, context)
    }
}

fn storage_reason(error: StorageError) -> WakeFailureReason {
    match error {
        StorageError::WakeRefused(reason) => WakeFailureReason::Refused(reason),
        StorageError::WakeNotFound => WakeFailureReason::ResourceNotFound,
        StorageError::OperationConflict => WakeFailureReason::OperationConflict,
        StorageError::Database(_) => WakeFailureReason::AutomationUnavailable,
        StorageError::InvalidTiming(error) => WakeFailureReason::InvalidField {
            field: "timing".into(),
            constraint: error.to_string(),
        },
        StorageError::InvalidWake { field, reason } => WakeFailureReason::InvalidField {
            field: field.into(),
            constraint: reason.into(),
        },
        _ => WakeFailureReason::InvalidRecord,
    }
}
