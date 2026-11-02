//! Typed schedule administration; operation identity recovers uncertain local mutations.
use crate::{CatalogMutationResult, CentralCatalogFailure, ClientError, CollaborationClient};
use collaboration_protocol::{
    OperationId, ScheduleCreateRequest, ScheduleEnableRequest, ScheduleFailure,
    ScheduleFailureKind, ScheduleShowRequest, ScheduleSnapshot, ScheduleUpdateRequest,
};
#[derive(Debug, thiserror::Error)]
pub enum ScheduleClientError {
    #[error(transparent)]
    Selection(Box<crate::ScheduleSelectionFailure>),
    #[error(transparent)]
    ReadFailure(Box<crate::CatalogReadFailure>),
    #[error(transparent)]
    Catalog(CentralCatalogFailure),
    #[error("schedule operation rejected: {0:?}")]
    Rejected(Box<ScheduleFailure>),
    #[error(transparent)]
    Connection(#[from] ClientError),
}
impl CollaborationClient {
    pub async fn import_schedule(
        &self,
        request: collaboration_protocol::ScheduleImportRequest,
    ) -> Result<CatalogMutationResult, ScheduleClientError> {
        self.schedule_call("schedule_import", request).await
    }
    pub async fn export_schedule(
        &self,
        request: ScheduleShowRequest,
    ) -> Result<collaboration_protocol::ScheduleExportResult, ScheduleClientError> {
        self.schedule_call("schedule_export", request).await
    }
    pub async fn create_schedule(
        &self,
        request: ScheduleCreateRequest,
    ) -> Result<crate::CentralScheduleSnapshot, ScheduleClientError> {
        self.schedule_call("schedule_create", request).await
    }
    pub async fn update_schedule(
        &self,
        request: ScheduleUpdateRequest,
    ) -> Result<crate::ScheduleMutationResponse, ScheduleClientError> {
        self.schedule_call("schedule_update", request).await
    }
    pub async fn read_schedule(
        &self,
        request: ScheduleShowRequest,
    ) -> Result<crate::ScheduleReadResponse, ScheduleClientError> {
        let expected = request.schedule_id.clone();
        let response = self.schedule_call("schedule_show", request).await?;
        let matches = match &response {
            crate::ScheduleReadResponse::Suspended { suspended, .. } => {
                suspended.kind == collaboration_protocol::AutomationKind::Schedule
                    && suspended.automation_id.as_str() == expected.as_str()
            }
            crate::ScheduleReadResponse::Local { value } => value.schedule_id == expected,
            crate::ScheduleReadResponse::Schedule { value } => value.schedule_id == expected,
            crate::ScheduleReadResponse::Faulted { fault, .. } => {
                fault.kind == collaboration_protocol::AutomationKind::Schedule
                    && fault.automation_id.as_str() == expected.as_str()
            }
        };
        if !matches {
            return Err(ClientError::Protocol("Schedule read identity mismatch").into());
        }
        Ok(response)
    }
    pub async fn enable_schedule(
        &self,
        request: ScheduleEnableRequest,
    ) -> Result<crate::ScheduleMutationResponse, ScheduleClientError> {
        self.schedule_call("schedule_enable", request).await
    }
    pub async fn disable_schedule(
        &self,
        request: ScheduleEnableRequest,
    ) -> Result<crate::ScheduleMutationResponse, ScheduleClientError> {
        self.schedule_call("schedule_disable", request).await
    }
    pub async fn prepare_schedule(
        &self,
        request: collaboration_protocol::SchedulePrepareRequest,
    ) -> Result<ScheduleSnapshot, ScheduleClientError> {
        self.schedule_call("schedule_prepare", request).await
    }
    async fn schedule_call<TRequest: serde::Serialize, TResult: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        request: TRequest,
    ) -> Result<TResult, ScheduleClientError> {
        let params = serde_json::to_value(request)
            .map_err(|_| ClientError::Protocol("invalid schedule request"))?;
        let shed = crate::admission_overload::ShedRequest::of(&params);
        let schedule_id = params
            .get("scheduleId")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let operation_id: Option<OperationId> = params
            .get("operationId")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| ClientError::Protocol("invalid schedule operation identity"))?;
        if method == "schedule_import" {
            let encoded_bytes = serde_json::to_vec(&params)
                .map(|bytes| bytes.len())
                .map_err(|_| ClientError::Protocol("invalid schedule request"))?;
            if encoded_bytes > collaboration_protocol::MAX_CONTROL_FRAME_BYTES {
                let operation_id = params
                    .get("operationId")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok());
                return Err(ScheduleClientError::Rejected(Box::new(
                    ScheduleFailure::package_frame_limit(operation_id, encoded_bytes),
                )));
            }
        }
        let result = match self.connection.call(method, params).await {
            Ok(result) => result,
            Err(ClientError::Rejected {
                code: -32050,
                data: Some(data),
            }) => {
                if let Ok(error) =
                    serde_json::from_value::<crate::ScheduleSelectionFailure>(data.clone())
                {
                    if error.operation_id() != operation_id.as_ref()
                        || error.schedule_id().map(|id| id.as_str()) != schedule_id.as_deref()
                    {
                        return Err(ClientError::Protocol(
                            "Schedule selection failure identity mismatch",
                        )
                        .into());
                    }
                    return Err(ScheduleClientError::Selection(Box::new(error)));
                }
                if let Ok(error) = serde_json::from_value::<crate::CatalogReadFailure>(data.clone())
                {
                    if !matches!(method, "schedule_export" | "schedule_show")
                        || error.kind() != collaboration_protocol::AutomationKind::Schedule
                        || schedule_id.as_deref() != Some(error.automation_id().as_str())
                    {
                        return Err(ClientError::Protocol(
                            "catalog Schedule fault identity mismatch",
                        )
                        .into());
                    }
                    return Err(ScheduleClientError::ReadFailure(Box::new(error)));
                }
                if let Ok(error) = serde_json::from_value::<CentralCatalogFailure>(data.clone()) {
                    if error.operation_id() != operation_id.as_ref() {
                        return Err(ClientError::Protocol(
                            "catalog schedule failure operation identity mismatch",
                        )
                        .into());
                    }
                    return Err(ScheduleClientError::Catalog(error));
                }
                let failure: Box<ScheduleFailure> = serde_json::from_value(data)
                    .map_err(|_| ClientError::Protocol("invalid schedule failure"))?;
                if matches!(failure.kind, ScheduleFailureKind::Overloaded)
                    || matches!(
                        method,
                        "schedule_prepare"
                            | "schedule_import"
                            | "schedule_export"
                            | "schedule_update"
                            | "schedule_enable"
                            | "schedule_disable"
                            | "schedule_show"
                    )
                {
                    return Err(ScheduleClientError::Rejected(failure));
                }
                return Err(ClientError::Protocol(
                    "catalog schedule returned a local mutation failure",
                )
                .into());
            }
            Err(ClientError::Overloaded { message }) => {
                return Err(ScheduleClientError::Rejected(
                    crate::admission_overload::schedule(message, &shed).into(),
                ));
            }
            Err(error) => return Err(error.into()),
        };
        serde_json::from_value(result)
            .map_err(|_| ClientError::Protocol("invalid schedule result").into())
    }
}
