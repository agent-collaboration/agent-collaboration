//! Controls follow durable ownership; a failed central call never falls back to local state.
use super::{WakeFailureContext, WakeOperations};
use crate::{
    CentralCatalogFailure,
    catalog_results::{CatalogMutationResult, WakeMutationResponse},
    wake_operation_failure::{WakeOperationFailure, WakeSelectionFailure},
};
use automation_storage::{StorageError, StoredAutomationOwner, WakeAction, WakeMutation};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationKind, CatalogAutomation, SavedMessage,
    WakeMutationRequest,
};

impl WakeOperations {
    pub async fn wake_send(
        &self,
        request: collaboration_protocol::WakeSendRequest,
    ) -> Result<crate::catalog_results::WakeCreateResponse, WakeOperationFailure> {
        if let Some(value) = self.recover_legacy_create(&request).await? {
            return Ok(crate::catalog_results::WakeCreateResponse::Legacy { value });
        }
        let operation = request.operation_id.clone();
        match self
            .catalog
            .request(
                collaboration_protocol::AutomationCall::WakeSend(request),
                Some(operation.clone()),
            )
            .await?
        {
            collaboration_protocol::AutomationAnswer::Wake(value) => {
                Ok(crate::catalog_results::WakeCreateResponse::Central { value })
            }
            _ => Err(crate::CentralCatalogFailure::unanswered(Some(operation)).into()),
        }
    }

    pub async fn wake_pause(
        &self,
        request: WakeMutationRequest,
    ) -> Result<WakeMutationResponse, WakeOperationFailure> {
        self.mutate_command(request, WakeAction::Pause).await
    }
    pub async fn wake_resume(
        &self,
        request: WakeMutationRequest,
    ) -> Result<WakeMutationResponse, WakeOperationFailure> {
        self.mutate_command(request, WakeAction::Resume).await
    }
    pub async fn wake_cancel(
        &self,
        request: WakeMutationRequest,
    ) -> Result<WakeMutationResponse, WakeOperationFailure> {
        self.mutate_command(request, WakeAction::Cancel).await
    }

    async fn mutate_command(
        &self,
        request: WakeMutationRequest,
        action: WakeAction,
    ) -> Result<WakeMutationResponse, WakeOperationFailure> {
        if let Some(value) = self.recover_legacy_mutation(&request, action).await? {
            return Ok(WakeMutationResponse::Local(value));
        }
        let unavailable = |message: String| {
            WakeOperationFailure::Selection(WakeSelectionFailure::LocalStoreUnavailable {
                operation_id: Some(request.operation_id.clone()),
                wakeup_id: Some(request.wakeup_id.clone()),
                message,
            })
        };
        let pending = || {
            WakeOperationFailure::Selection(WakeSelectionFailure::HandoverPending {
                operation_id: request.operation_id.clone(),
                wakeup_id: request.wakeup_id.clone(),
                message: "Wake handover is pending; this attempt did not change the local Wake or dispatch a central mutation."
                    .into(),
            })
        };
        let store = self.store.as_ref().ok_or_else(|| {
            unavailable(
                "Local Wake ownership storage is unavailable; no mutation was dispatched.".into(),
            )
        })?;
        let local_result = {
            let mut store = store.lock().await;
            match store.wake_command_owner(&request.wakeup_id).await
                .map_err(|error| unavailable(format!("Local Wake ownership selection failed: {error}; no mutation was dispatched.")))?
            {
                StoredAutomationOwner::HandoverPending => return Err(pending()),
                StoredAutomationOwner::Central => None,
                StoredAutomationOwner::Local => Some(store.mutate_wakeup::<SavedMessage>(&WakeMutation {
                    operation_id: request.operation_id.clone(), wakeup_id: request.wakeup_id.clone(), action,
                    now_ms: chrono::Utc::now().timestamp_millis(),
                }).await),
            }
        };
        match local_result {
            Some(Err(StorageError::AutomationHandoverPending)) => return Err(pending()),
            Some(Err(StorageError::AutomationOwnedByCentral)) | None => {}
            Some(result) => {
                return self
                    .project_local_mutation(
                        result,
                        WakeFailureContext {
                            operation_id: Some(request.operation_id),
                            wakeup_id: Some(request.wakeup_id),
                        },
                    )
                    .map(WakeMutationResponse::Local)
                    .map_err(WakeOperationFailure::from);
            }
        }
        // A transaction that observed central ownership made no local write. Dispatch once
        // to that owner with the caller's original operation identity.
        let operation = request.operation_id.clone();
        let id = request.wakeup_id.clone();
        let call = match action {
            WakeAction::Pause => AutomationCall::WakePause(request),
            WakeAction::Resume => AutomationCall::WakeResume(request),
            WakeAction::Cancel => AutomationCall::WakeCancel(request),
        };
        match self.catalog.request(call, Some(operation.clone())).await? {
            AutomationAnswer::Mutation {
                automation,
                already_sent,
            } if match &automation {
                CatalogAutomation::Wake(wake) => wake.definition.wakeup_id == id,
                CatalogAutomation::Faulted(fault) => {
                    fault.kind == AutomationKind::Wake
                        && fault.automation_id.as_str() == id.as_str()
                }
                CatalogAutomation::Schedule(_) => false,
                CatalogAutomation::Suspended(value) => {
                    value.kind == AutomationKind::Wake
                        && value.automation_id.as_str() == id.as_str()
                }
            } =>
            {
                Ok(WakeMutationResponse::Central(CatalogMutationResult {
                    automation: automation.into(),
                    already_sent,
                }))
            }
            _ => Err(CentralCatalogFailure::unanswered(Some(operation)).into()),
        }
    }
}
