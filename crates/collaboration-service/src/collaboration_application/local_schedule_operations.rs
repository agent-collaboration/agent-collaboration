//! Unmoved Schedule commands keep the existing local validation, receipts and Run projection.
use super::AutomationOperations;
use crate::schedule_command_failure::{
    ScheduleFailureContext, ScheduleOperationFailure, schedule_failure,
};
use crate::{ScheduleCommandFailure, ScheduleSelectionFailure};
use automation_storage::{
    ScheduleEdit, ScheduleInspection, ScheduleMutation, StorageError, StoredAutomationOwner,
};
use collaboration_protocol::{
    AutomationCall, EndpointRef, LocalMutationState, OperationId, ScheduleId, ScheduleSnapshot,
    SessionRef,
};

fn selection(
    context: &ScheduleFailureContext,
    detail: impl std::fmt::Display,
) -> ScheduleCommandFailure {
    ScheduleSelectionFailure::LocalStoreUnavailable {
        operation_id: context.operation_id.clone(),
        schedule_id: context.schedule_id.clone(),
        message: format!(
            "Local Schedule ownership selection failed: {detail}; no central fallback was used."
        ),
    }
    .into()
}
pub(super) fn handover_pending(operation: &OperationId, id: &ScheduleId) -> ScheduleCommandFailure {
    ScheduleSelectionFailure::HandoverPending {
        operation_id:operation.clone(),schedule_id:id.clone(),
        message:"Schedule handover is pending; this attempt changed no local state and dispatched no central mutation.".into(),
    }.into()
}
impl AutomationOperations {
    pub(super) async fn schedule_preparation_owner(
        &self,
        operation: &OperationId,
        id: &ScheduleId,
    ) -> Result<StoredAutomationOwner, ScheduleCommandFailure> {
        let context = ScheduleFailureContext::new(Some(operation.clone()), Some(id.clone()));
        let store = self
            .store()
            .ok_or_else(|| selection(&context, "storage unavailable"))?;
        let owner = store
            .lock()
            .await
            .schedule_command_owner(id)
            .await
            .map_err(|error| selection(&context, error))?;
        if owner == StoredAutomationOwner::HandoverPending {
            Err(handover_pending(operation, id))
        } else {
            Ok(owner)
        }
    }

    pub(super) async fn local_schedule_snapshot(
        &self,
        id: &ScheduleId,
    ) -> Result<Option<ScheduleSnapshot>, ScheduleCommandFailure> {
        let context = ScheduleFailureContext::new(None, Some(id.clone()));
        let store = self
            .store()
            .ok_or_else(|| selection(&context, "storage unavailable"))?;
        let mut store = store.lock().await;
        match store
            .schedule_command_owner(id)
            .await
            .map_err(|error| selection(&context, error))?
        {
            StoredAutomationOwner::Central => Ok(None),
            StoredAutomationOwner::Local | StoredAutomationOwner::HandoverPending => {
                let result = store.inspect_schedule::<SessionRef, EndpointRef>(id).await;
                project_local_result(result, context, false, None).map(Some)
            }
        }
    }

    pub(super) async fn try_local_schedule_mutation(
        &self,
        call: &AutomationCall,
        operation: &OperationId,
        id: &ScheduleId,
    ) -> Result<Option<ScheduleSnapshot>, ScheduleCommandFailure> {
        let context = ScheduleFailureContext::new(Some(operation.clone()), Some(id.clone()));
        let store = self
            .store()
            .ok_or_else(|| selection(&context, "storage unavailable"))?;
        match store
            .lock()
            .await
            .schedule_command_owner(id)
            .await
            .map_err(|error| selection(&context, error))?
        {
            StoredAutomationOwner::Central => return Ok(None),
            StoredAutomationOwner::HandoverPending => return Err(handover_pending(operation, id)),
            StoredAutomationOwner::Local => {}
        }
        let edit = match call {
            AutomationCall::ScheduleUpdate(request) => ScheduleEdit::Replace {
                expected_change_id: request.expected_change_id.clone(),
                definition: serde_json::to_value(&request.definition)
                    .and_then(serde_json::from_value)
                    .map_err(|_| {
                        ScheduleOperationFailure::Operation(schedule_failure(
                            context.clone(),
                            StorageError::InvalidSchedule {
                                field: "definition",
                                reason: "Use the retained local Schedule definition shape.",
                            },
                            LocalMutationState::None,
                            None,
                        ))
                    })?,
            },
            AutomationCall::ScheduleEnable(_) => ScheduleEdit::SetEnabled { enabled: true },
            AutomationCall::ScheduleDisable(_) => ScheduleEdit::SetEnabled { enabled: false },
            _ => return Err(selection(&context, "invalid local command")),
        };
        let definition = match &edit {
            ScheduleEdit::Replace { definition, .. } => definition.clone(),
            ScheduleEdit::SetEnabled { enabled } => {
                let inspected = store
                    .lock()
                    .await
                    .inspect_schedule::<SessionRef, EndpointRef>(id)
                    .await;
                let inspected = inspected.map_err(|error| {
                    ScheduleOperationFailure::Operation(schedule_failure(
                        context.clone(),
                        error,
                        LocalMutationState::None,
                        None,
                    ))
                })?;
                let mut definition = inspected.record.definition;
                definition.enabled = *enabled;
                definition
            }
        };
        crate::schedule_activation::validate(
            store,
            crate::schedule_activation::ActivationRequest {
                definition: &definition,
                schedule_id: Some(id),
                operation_id: operation,
                service_id: self.identity.service_id(),
                execution: self.scheduled_run_execution.as_ref(),
            },
        )
        .await
        .map_err(|error| {
            ScheduleOperationFailure::Operation(schedule_failure(
                context.clone(),
                error,
                LocalMutationState::None,
                None,
            ))
        })?;
        let result = store
            .lock()
            .await
            .mutate_schedule(&ScheduleMutation {
                operation_id: operation.clone(),
                schedule_id: id.clone(),
                edit,
                now_ms: chrono::Utc::now().timestamp_millis(),
            })
            .await;
        match result {
            Err(StorageError::AutomationOwnedByCentral) => Ok(None),
            Err(StorageError::AutomationHandoverPending) => Err(handover_pending(operation, id)),
            result => {
                let current = if matches!(result, Err(StorageError::ScheduleChangeConflict)) {
                    store
                        .lock()
                        .await
                        .inspect_schedule::<SessionRef, EndpointRef>(id)
                        .await
                        .ok()
                        .map(|value| value.record.change_id)
                } else {
                    None
                };
                project_local_result(result, context, true, current).map(Some)
            }
        }
    }
}
fn project_local_result(
    result: Result<ScheduleInspection<SessionRef, EndpointRef>, StorageError>,
    context: ScheduleFailureContext,
    mutating: bool,
    current: Option<collaboration_protocol::ChangeId>,
) -> Result<ScheduleSnapshot, ScheduleCommandFailure> {
    match result {
        Ok(value) => crate::schedule_projection::snapshot(value).map_err(|()| {
            ScheduleOperationFailure::Operation(schedule_failure(
                context,
                StorageError::InvalidRecord,
                if mutating {
                    LocalMutationState::Committed
                } else {
                    LocalMutationState::None
                },
                current,
            ))
            .into()
        }),
        Err(error) => {
            let mutation = if mutating && matches!(error, StorageError::Database(_)) {
                LocalMutationState::Unknown
            } else {
                LocalMutationState::None
            };
            Err(ScheduleOperationFailure::Operation(schedule_failure(
                context, error, mutation, current,
            ))
            .into())
        }
    }
}
