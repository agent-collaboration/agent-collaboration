//! Schedule commands select durable local custody or central catalog ownership.
use super::automation_operations::AutomationOperations;
use crate::{
    CatalogMutationResult, CentralCatalogFailure, CentralScheduleSnapshot, ScheduleCommandFailure,
    ScheduleMutationResponse, ScheduleReadResponse,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationKind, CatalogAutomation, OperationId,
    ScheduleCreateRequest, ScheduleEnableRequest, ScheduleId, ScheduleShowRequest,
    ScheduleUpdateRequest,
};
impl AutomationOperations {
    pub async fn schedule_create(
        &self,
        request: ScheduleCreateRequest,
    ) -> Result<CentralScheduleSnapshot, CentralCatalogFailure> {
        let operation = request.operation_id.clone();
        match self
            .catalog
            .request(
                AutomationCall::ScheduleCreate(request),
                Some(operation.clone()),
            )
            .await?
        {
            AutomationAnswer::Schedule(value) => Ok(value.into()),
            _ => Err(CentralCatalogFailure::unanswered(Some(operation))),
        }
    }
    pub async fn schedule_show(
        &self,
        request: ScheduleShowRequest,
    ) -> Result<ScheduleReadResponse, ScheduleCommandFailure> {
        let id = request.schedule_id.clone();
        if let Some(value) = self.local_schedule_snapshot(&id).await? {
            return Ok(ScheduleReadResponse::Local { value });
        }
        let automation = match self
            .catalog
            .request(AutomationCall::ScheduleShow(request), None)
            .await?
        {
            AutomationAnswer::Suspended(value) => CatalogAutomation::Suspended(value),
            AutomationAnswer::Schedule(value) => CatalogAutomation::Schedule(value),
            AutomationAnswer::Faulted(value) => CatalogAutomation::Faulted(value),
            _ => return Err(CentralCatalogFailure::InvalidReply.into()),
        };
        if !matches_schedule(&automation, &id) {
            return Err(CentralCatalogFailure::InvalidReply.into());
        }
        ScheduleReadResponse::from_catalog(automation)
            .ok_or_else(|| CentralCatalogFailure::InvalidReply.into())
    }
    pub async fn schedule_update(
        &self,
        request: ScheduleUpdateRequest,
    ) -> Result<ScheduleMutationResponse, ScheduleCommandFailure> {
        self.change_selected_schedule(
            request.operation_id.clone(),
            request.schedule_id.clone(),
            AutomationCall::ScheduleUpdate(request),
        )
        .await
    }
    pub async fn schedule_enable(
        &self,
        request: ScheduleEnableRequest,
    ) -> Result<ScheduleMutationResponse, ScheduleCommandFailure> {
        self.change_selected_schedule(
            request.operation_id.clone(),
            request.schedule_id.clone(),
            AutomationCall::ScheduleEnable(request),
        )
        .await
    }
    pub async fn schedule_disable(
        &self,
        request: ScheduleEnableRequest,
    ) -> Result<ScheduleMutationResponse, ScheduleCommandFailure> {
        self.change_selected_schedule(
            request.operation_id.clone(),
            request.schedule_id.clone(),
            AutomationCall::ScheduleDisable(request),
        )
        .await
    }
    async fn change_selected_schedule(
        &self,
        operation: OperationId,
        id: ScheduleId,
        call: AutomationCall,
    ) -> Result<ScheduleMutationResponse, ScheduleCommandFailure> {
        if let Some(value) = self
            .try_local_schedule_mutation(&call, &operation, &id)
            .await?
        {
            return Ok(ScheduleMutationResponse::Local(value));
        }
        self.change_schedule(operation, id, call)
            .await
            .map(ScheduleMutationResponse::Central)
            .map_err(Into::into)
    }
    pub(super) async fn change_schedule(
        &self,
        operation: OperationId,
        id: ScheduleId,
        call: AutomationCall,
    ) -> Result<CatalogMutationResult, CentralCatalogFailure> {
        match self.catalog.request(call, Some(operation.clone())).await? {
            AutomationAnswer::Mutation {
                automation,
                already_sent,
            } if matches_schedule(&automation, &id) => Ok(CatalogMutationResult {
                automation: automation.into(),
                already_sent,
            }),
            _ => Err(CentralCatalogFailure::unanswered(Some(operation))),
        }
    }
}
fn matches_schedule(automation: &CatalogAutomation, id: &ScheduleId) -> bool {
    match automation {
        CatalogAutomation::Suspended(value) => {
            value.kind == AutomationKind::Schedule && value.automation_id.as_str() == id.as_str()
        }
        CatalogAutomation::Schedule(value) => value.schedule_id == *id,
        CatalogAutomation::Faulted(value) => {
            value.kind == AutomationKind::Schedule && value.automation_id.as_str() == id.as_str()
        }
        CatalogAutomation::Wake(_) => false,
    }
}
