//! Interim old-operation reads only; remove after the 32-day retention following release.
use super::{WakeFailureContext, WakeOperations, wake_failure};
use crate::wake_operation_failure::{WakeOperationFailure, WakeSelectionFailure};
use automation_storage::{RecordedRemoteWake, WakeAction};
use collaboration_mail_protocol::{
    RemoteWakeCreator, RemoteWakeRequest, RemoteWakeResult, WakeBody,
};
use collaboration_protocol::{
    LocalMutationState, OperationId, WakeFailureReason, WakeMutationRequest, WakeMutationResult,
    WakeSendRequest, WakeSnapshot, WakeupId,
};
use serde::{Serialize, de::DeserializeOwned};

fn convert<Value: DeserializeOwned>(value: impl Serialize) -> Result<Value, ()> {
    serde_json::from_value(serde_json::to_value(value).map_err(|_| ())?).map_err(|_| ())
}
fn unknown(context: &WakeFailureContext) -> WakeOperationFailure {
    wake_failure(
        context.clone(),
        WakeFailureReason::RemoteUnavailable,
        LocalMutationState::Unknown,
    )
    .into()
}
fn invalid(context: &WakeFailureContext) -> WakeOperationFailure {
    wake_failure(
        context.clone(),
        WakeFailureReason::InvalidRecord,
        LocalMutationState::Unknown,
    )
    .into()
}
fn conflict(context: &WakeFailureContext) -> WakeOperationFailure {
    wake_failure(
        context.clone(),
        WakeFailureReason::OperationConflict,
        LocalMutationState::None,
    )
    .into()
}

impl WakeOperations {
    async fn recorded_legacy_request(
        &self,
        operation: &OperationId,
        wakeup: Option<&WakeupId>,
    ) -> Result<Option<RecordedRemoteWake>, WakeOperationFailure> {
        let unavailable = || {
            WakeOperationFailure::Selection(WakeSelectionFailure::LocalStoreUnavailable {
                operation_id: Some(operation.clone()),
                wakeup_id: wakeup.cloned(),
                message:
                    "Old Wake operation history could not be inspected; no mutation was dispatched."
                        .into(),
            })
        };
        let store = self.store.as_ref().ok_or_else(unavailable)?;
        store
            .lock()
            .await
            .recorded_remote_wake(operation)
            .await
            .map_err(|_| unavailable())
    }

    async fn recorded_legacy_answer(
        &self,
        recorded: &RecordedRemoteWake,
        context: &WakeFailureContext,
    ) -> Result<RemoteWakeResult, WakeOperationFailure> {
        if let Some(snapshot) = &recorded.snapshot {
            return convert(snapshot)
                .map(RemoteWakeResult::Snapshot)
                .map_err(|()| invalid(context));
        }
        let answer = self
            .legacy_wake_recovery
            .as_ref()
            .ok_or_else(|| unknown(context))?
            .recorded_wake_answer(&recorded.request.operation_id)
            .await
            .map_err(|()| unknown(context))?
            .ok_or_else(|| unknown(context))?;
        if let RemoteWakeResult::Refused { reason, mutation } = answer {
            let reason = convert(reason).map_err(|()| invalid(context))?;
            let mutation = convert(mutation).map_err(|()| invalid(context))?;
            return Err(wake_failure(context.clone(), reason, mutation).into());
        }
        Ok(answer)
    }

    pub(super) async fn recover_legacy_create(
        &self,
        request: &WakeSendRequest,
    ) -> Result<Option<WakeSnapshot>, WakeOperationFailure> {
        let Some(recorded) = self
            .recorded_legacy_request(&request.operation_id, None)
            .await?
        else {
            return Ok(None);
        };
        let context = WakeFailureContext::operation(request.operation_id.clone());
        let canonical = serde_json::to_vec(request).map_err(|_| invalid(&context))?;
        if recorded.request.canonical_request != canonical {
            return Err(conflict(&context));
        }
        if !matches!(&recorded.request.body.request, RemoteWakeRequest::Send { operation_id, .. }
            if String::from(operation_id.clone()) == request.operation_id.as_str())
        {
            return Err(invalid(&context));
        }
        let RemoteWakeResult::Snapshot(snapshot) =
            self.recorded_legacy_answer(&recorded, &context).await?
        else {
            return Err(invalid(&context));
        };
        let snapshot: WakeSnapshot = convert(*snapshot).map_err(|()| invalid(&context))?;
        if snapshot.definition.message.target != request.message.target {
            return Err(invalid(&context));
        }
        Ok(Some(snapshot))
    }

    pub(super) async fn recover_legacy_mutation(
        &self,
        request: &WakeMutationRequest,
        action: WakeAction,
    ) -> Result<Option<WakeMutationResult>, WakeOperationFailure> {
        let Some(recorded) = self
            .recorded_legacy_request(&request.operation_id, Some(&request.wakeup_id))
            .await?
        else {
            return Ok(None);
        };
        let context = WakeFailureContext {
            operation_id: Some(request.operation_id.clone()),
            wakeup_id: Some(request.wakeup_id.clone()),
        };
        let operation_id = convert(&request.operation_id).map_err(|()| invalid(&context))?;
        let wakeup_id = convert(&request.wakeup_id).map_err(|()| invalid(&context))?;
        let request_body = match action {
            WakeAction::Pause => RemoteWakeRequest::Pause {
                operation_id,
                wakeup_id,
            },
            WakeAction::Resume => RemoteWakeRequest::Resume {
                operation_id,
                wakeup_id,
            },
            WakeAction::Cancel => RemoteWakeRequest::Cancel {
                operation_id,
                wakeup_id,
            },
        };
        let body = WakeBody {
            requested_by: RemoteWakeCreator::Owner {},
            request: request_body,
        };
        let canonical = serde_json::to_vec(&(&recorded.request.target, &body))
            .map_err(|_| invalid(&context))?;
        if recorded.request.canonical_request != canonical {
            return Err(conflict(&context));
        }
        if serde_json::to_value(&recorded.request.body).map_err(|_| invalid(&context))?
            != serde_json::to_value(&body).map_err(|_| invalid(&context))?
        {
            return Err(invalid(&context));
        }
        let RemoteWakeResult::Mutation(result) =
            self.recorded_legacy_answer(&recorded, &context).await?
        else {
            return Err(invalid(&context));
        };
        let result: WakeMutationResult = convert(*result).map_err(|()| invalid(&context))?;
        if result.wakeup.definition.wakeup_id != request.wakeup_id
            || result.wakeup.definition.message.target != recorded.request.target
        {
            return Err(invalid(&context));
        }
        Ok(Some(result))
    }
}
