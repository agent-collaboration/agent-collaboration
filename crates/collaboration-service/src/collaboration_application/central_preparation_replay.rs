//! Stored Bind replay and finalization preserve central answers and native-target evidence.
use super::AutomationOperations;
use super::central_schedule_preparation::{Evidence, PreparationResult, invalid, storage_failure};
use crate::schedule_preparation::error;
use crate::{CentralCatalogFailure, collaboration_application::ScheduleOperationFailure};
use automation_storage::{ExternalOperationRecord, StorageError, StoredCentralBindRequest};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationRefusal, CatalogAutomation, EndpointRef,
    ScheduleFailureKind, SchedulePrepareRequest, SessionRef,
};

impl AutomationOperations {
    pub(super) async fn replay_central_preparation(
        &self,
        params: &SchedulePrepareRequest,
        record: ExternalOperationRecord<
            Evidence,
            automation_storage::ScheduleInspection<SessionRef, EndpointRef>,
        >,
    ) -> PreparationResult {
        if let Some(result) = record.result {
            return crate::schedule_projection::snapshot(result).map_err(|()| {
                invalid(
                    params,
                    "Stored preparation result is inconsistent; inspect this operation.",
                )
            });
        }
        if record.status == "inProgress"
            && let Some(bind) = record.central_bind_request
        {
            return self
                .dispatch_retained_bind(params, &bind, record.evidence)
                .await;
        }
        if let Some(failure) = record.failure {
            let failure =
                collaboration_runtime::schedule_preparation_evidence_sink::upgrade_stored_failure(
                    failure,
                )
                .and_then(|value| {
                    serde_json::from_value(value).map_err(|_| StorageError::InvalidRecord)
                })
                .map_err(|failure| storage_failure(params, failure))?;
            return Err(ScheduleOperationFailure::RetainedPreparation(failure).into());
        }
        Err(ScheduleOperationFailure::Preparation(error(params,ScheduleFailureKind::OutcomeUnknown,
            "Preparation was already admitted without a retained central Bind; inspect this operation rather than creating another native target.",record.evidence)).into())
    }

    pub(super) async fn dispatch_retained_bind(
        &self,
        params: &SchedulePrepareRequest,
        bind: &StoredCentralBindRequest,
        evidence: Evidence,
    ) -> PreparationResult {
        let request = bind
            .request()
            .map_err(|failure| storage_failure(params, failure))?;
        match self
            .catalog
            .request(
                AutomationCall::ScheduleBindPrepared(request),
                Some(params.operation_id.clone()),
            )
            .await
        {
            Ok(AutomationAnswer::Mutation {
                automation: CatalogAutomation::Schedule(answer),
                ..
            }) => self
                .store()
                .ok_or_else(|| {
                    invalid(params, "Automation storage unavailable after central Bind.")
                })?
                .lock()
                .await
                .complete_central_preparation(bind, answer)
                .await
                .and_then(|inspection| {
                    crate::schedule_projection::snapshot(inspection)
                        .map_err(|()| StorageError::InvalidRecord)
                })
                .map_err(|_| {
                    CentralCatalogFailure::OutcomeUnknown {
                        operation_id: params.operation_id.clone(),
                    }
                    .into()
                }),
            Err(CentralCatalogFailure::Refused { error: refusal, .. }) => {
                let (kind, current) = match &refusal {
                    AutomationRefusal::ScheduleChangeConflict { current_change_id } => (
                        ScheduleFailureKind::ChangeConflict,
                        Some(current_change_id.clone()),
                    ),
                    AutomationRefusal::OperationConflict { .. } => {
                        (ScheduleFailureKind::OperationConflict, None)
                    }
                    AutomationRefusal::NotFound => (ScheduleFailureKind::ResourceNotFound, None),
                    _ => (ScheduleFailureKind::InvalidField, None),
                };
                let mut failure = error(
                    params,
                    kind,
                    &format!(
                        "Central refused the retained Bind: {refusal:?}; inspect retained native effects."
                    ),
                    evidence.clone(),
                );
                failure.current_change_id = current;
                self.store()
                    .ok_or_else(|| {
                        invalid(
                            params,
                            "Automation storage unavailable after central refusal.",
                        )
                    })?
                    .lock()
                    .await
                    .record_preparation_failure(automation_storage::PreparationFailureRecord {
                        operation_id: params.operation_id.clone(),
                        schedule_id: params.schedule_id.clone(),
                        disposition: automation_storage::PreparationFailureDisposition::Failed,
                        evidence,
                        error: &failure,
                    })
                    .await
                    .map_err(|_| CentralCatalogFailure::OutcomeUnknown {
                        operation_id: params.operation_id.clone(),
                    })?;
                Err(ScheduleOperationFailure::Preparation(failure).into())
            }
            Err(failure) => Err(failure.into()),
            Ok(_) => Err(CentralCatalogFailure::OutcomeUnknown {
                operation_id: params.operation_id.clone(),
            }
            .into()),
        }
    }
}
