//! Bounded inspection methods share typed errors while preserving their distinct record types.
use crate::{
    AutomationInspectionClientError, CentralCatalogFailure, ClientError, CollaborationClient,
    FiringShowRequest,
};
use collaboration_protocol::{
    AutomationPage, AutomationPageRequest, DeliveryInspection, DeliveryListRequest,
    FiringListRequest, FiringSnapshot, InstructionSnapshot, RevisionListRequest, RevisionRecord,
    RunListRequest, RunSnapshot,
};

impl CollaborationClient {
    /// Observe the exact recorded worker/summary turn without starting or interrupting native work.
    pub async fn reconcile_run(
        &self,
        request: collaboration_protocol::RunShowRequest,
    ) -> Result<RunSnapshot, AutomationInspectionClientError> {
        let expected = request.run_id.clone();
        let result: RunSnapshot = self
            .automation_inspection_call("run_reconcile", request)
            .await?;
        if result.run_id != expected {
            return Err(ClientError::Protocol("Run reconciliation identity mismatch").into());
        }
        Ok(result)
    }
    /// Observe an uncertain delivery without resending input. Absence remains uncertain.
    pub async fn reconcile_delivery(
        &self,
        request: collaboration_protocol::DeliveryShowRequest,
    ) -> Result<DeliveryInspection, AutomationInspectionClientError> {
        let expected = request.delivery_id.clone();
        let result: DeliveryInspection = self
            .automation_inspection_call("delivery_reconcile", request)
            .await?;
        if result.delivery_id != expected {
            return Err(ClientError::Protocol("delivery reconciliation identity mismatch").into());
        }
        Ok(result)
    }
    pub async fn read_operation(
        &self,
        request: collaboration_protocol::OperationShowRequest,
    ) -> Result<collaboration_protocol::OperationSnapshot, AutomationInspectionClientError> {
        self.inspect_operation("operation_show", request).await
    }
    /// Reconcile original evidence without replaying native input or allocating a thread.
    pub async fn reconcile_operation(
        &self,
        request: collaboration_protocol::OperationShowRequest,
    ) -> Result<collaboration_protocol::OperationSnapshot, AutomationInspectionClientError> {
        self.inspect_operation("operation_reconcile", request).await
    }
    async fn inspect_operation(
        &self,
        method: &str,
        request: collaboration_protocol::OperationShowRequest,
    ) -> Result<collaboration_protocol::OperationSnapshot, AutomationInspectionClientError> {
        let expected = request.operation_id.clone();
        let result: collaboration_protocol::OperationSnapshot =
            self.automation_inspection_call(method, request).await?;
        if result.operation_id != expected || !result.has_consistent_outcome() {
            return Err(
                ClientError::Protocol("operation result identity or method mismatch").into(),
            );
        }
        Ok(result)
    }
    pub async fn read_automation_events(
        &self,
        request: collaboration_protocol::AutomationEventsRequest,
    ) -> Result<collaboration_protocol::AutomationEventsPage, AutomationInspectionClientError> {
        self.automation_inspection_call("automation_events", request)
            .await
    }
    pub async fn read_delivery_attempts(
        &self,
        request: collaboration_protocol::DeliveryAttemptsRequest,
    ) -> Result<
        collaboration_protocol::AttemptHistoryPage<collaboration_protocol::AttemptInspection>,
        AutomationInspectionClientError,
    > {
        self.automation_inspection_call("delivery_attempts", request)
            .await
    }
    pub async fn read_run_summaries(
        &self,
        request: collaboration_protocol::RunSummariesRequest,
    ) -> Result<
        collaboration_protocol::AttemptHistoryPage<collaboration_protocol::SummaryInspection>,
        AutomationInspectionClientError,
    > {
        self.automation_inspection_call("run_summaries", request)
            .await
    }
    pub async fn list_instructions(
        &self,
        request: AutomationPageRequest,
    ) -> Result<AutomationPage<InstructionSnapshot>, AutomationInspectionClientError> {
        self.automation_inspection_call("instruction_list", request)
            .await
    }
    pub async fn list_schedules(
        &self,
        request: AutomationPageRequest,
    ) -> Result<AutomationPage<crate::ScheduleReadResponse>, AutomationInspectionClientError> {
        let page: AutomationPage<crate::ScheduleReadResponse> = self
            .automation_inspection_call("schedule_list", request)
            .await?;
        if page.records.iter().any(|record| match record {
            crate::ScheduleReadResponse::Faulted { fault, .. } => {
                fault.kind != collaboration_protocol::AutomationKind::Schedule
            }
            crate::ScheduleReadResponse::Suspended { suspended, .. } => {
                suspended.kind != collaboration_protocol::AutomationKind::Schedule
            }
            crate::ScheduleReadResponse::Local { .. }
            | crate::ScheduleReadResponse::Schedule { .. } => false,
        }) {
            return Err(ClientError::Protocol("Schedule list fault kind mismatch").into());
        }
        Ok(page)
    }
    pub async fn list_runs(
        &self,
        request: RunListRequest,
    ) -> Result<AutomationPage<RunSnapshot>, AutomationInspectionClientError> {
        self.automation_inspection_call("run_list", request).await
    }
    pub async fn list_deliveries(
        &self,
        request: DeliveryListRequest,
    ) -> Result<AutomationPage<DeliveryInspection>, AutomationInspectionClientError> {
        self.automation_inspection_call("delivery_list", request)
            .await
    }
    pub async fn list_instruction_revisions(
        &self,
        request: RevisionListRequest,
    ) -> Result<AutomationPage<RevisionRecord>, AutomationInspectionClientError> {
        self.automation_inspection_call("revision_list", request)
            .await
    }
    pub(crate) async fn automation_inspection_call<
        TRequest: serde::Serialize,
        TResult: serde::de::DeserializeOwned,
    >(
        &self,
        method: &str,
        request: TRequest,
    ) -> Result<TResult, AutomationInspectionClientError> {
        let params = serde_json::to_value(request)
            .map_err(|_| ClientError::Protocol("invalid inspection request"))?;
        let result = match self.connection.call(method, params).await {
            Ok(result) => result,
            Err(ClientError::Rejected {
                code: -32050,
                data: Some(data),
            }) => {
                if let Ok(error) = serde_json::from_value::<CentralCatalogFailure>(data.clone()) {
                    if error.operation_id().is_some() {
                        return Err(ClientError::Protocol(
                            "catalog inspection failure supplied a mutation operation ID",
                        )
                        .into());
                    }
                    return Err(AutomationInspectionClientError::Catalog(error));
                }
                return Err(AutomationInspectionClientError::Rejected(
                    serde_json::from_value(data)
                        .map_err(|_| ClientError::Protocol("invalid inspection error"))?,
                ));
            }
            Err(ClientError::Overloaded { message }) => {
                return Err(AutomationInspectionClientError::Rejected(
                    crate::admission_overload::inspection(message).into(),
                ));
            }
            Err(error) => return Err(error.into()),
        };
        match serde_json::from_value(result) {
            Ok(result) => Ok(result),
            Err(_) => Err(ClientError::Protocol("invalid inspection response").into()),
        }
    }
}

impl CollaborationClient {
    pub async fn read_firing(
        &self,
        request: FiringShowRequest,
    ) -> Result<FiringSnapshot, AutomationInspectionClientError> {
        let expected = request.firing_id.clone();
        let firing: FiringSnapshot = self
            .automation_inspection_call("firing_show", request)
            .await?;
        if firing.firing_id != expected {
            return Err(ClientError::Protocol("Firing read identity mismatch").into());
        }
        Ok(firing)
    }

    pub async fn list_firings(
        &self,
        request: FiringListRequest,
    ) -> Result<AutomationPage<FiringSnapshot>, AutomationInspectionClientError> {
        let expected = request.automation_id.clone();
        let page: AutomationPage<FiringSnapshot> = self
            .automation_inspection_call("firing_list", request)
            .await?;
        if page
            .records
            .iter()
            .any(|firing| firing.automation_id != expected)
        {
            return Err(ClientError::Protocol("Firing list automation identity mismatch").into());
        }
        Ok(page)
    }
}
