//! Reminder operations return durable wake identity, never an implied native submission receipt.
use crate::{CentralCatalogFailure, ClientError, CollaborationClient};
use collaboration_protocol::{OperationId, WakeFailure, WakeSendRequest, WakeShowRequest};
use serde::{Serialize, de::DeserializeOwned};
#[derive(Debug, thiserror::Error)]
pub enum WakeClientError {
    #[error(transparent)]
    Selection(Box<crate::WakeSelectionFailure>),
    #[error(transparent)]
    Catalog(CentralCatalogFailure),
    #[error("wake operation rejected: {0:?}")]
    Rejected(Box<WakeFailure>),
    #[error(transparent)]
    Connection(#[from] ClientError),
}
impl CollaborationClient {
    pub async fn send_wakeup(
        &self,
        request: WakeSendRequest,
    ) -> Result<crate::WakeCreateResponse, WakeClientError> {
        self.wakeup_call("wake_send", request).await
    }
    pub async fn read_wakeup(
        &self,
        request: WakeShowRequest,
    ) -> Result<crate::WakeReadResponse, WakeClientError> {
        let expected = request.wakeup_id.clone();
        let response = self.wakeup_call("wake_show", request).await?;
        let matches = match &response {
            crate::WakeReadResponse::Suspended { suspended, .. } => {
                suspended.kind == collaboration_protocol::AutomationKind::Wake
                    && suspended.automation_id.as_str() == expected.as_str()
            }
            crate::WakeReadResponse::Local { value } => value.definition.wakeup_id == expected,
            crate::WakeReadResponse::Central { value } => value.definition.wakeup_id == expected,
            crate::WakeReadResponse::Faulted { fault, .. } => {
                fault.kind == collaboration_protocol::AutomationKind::Wake
                    && fault.automation_id.as_str() == expected.as_str()
            }
        };
        if !matches {
            return Err(ClientError::Protocol("Wake read identity mismatch").into());
        }
        Ok(response)
    }
    pub async fn pause_wakeup(
        &self,
        request: collaboration_protocol::WakeMutationRequest,
    ) -> Result<crate::WakeMutationResponse, WakeClientError> {
        self.wakeup_call("wake_pause", request).await
    }
    pub async fn resume_wakeup(
        &self,
        request: collaboration_protocol::WakeMutationRequest,
    ) -> Result<crate::WakeMutationResponse, WakeClientError> {
        self.wakeup_call("wake_resume", request).await
    }
    pub async fn cancel_wakeup(
        &self,
        request: collaboration_protocol::WakeMutationRequest,
    ) -> Result<crate::WakeMutationResponse, WakeClientError> {
        self.wakeup_call("wake_cancel", request).await
    }
    pub async fn read_delivery(
        &self,
        request: collaboration_protocol::DeliveryShowRequest,
    ) -> Result<collaboration_protocol::DeliveryInspection, WakeClientError> {
        self.wakeup_call("delivery_show", request).await
    }
    pub async fn list_wakeups(
        &self,
        request: collaboration_protocol::AutomationPageRequest,
    ) -> Result<collaboration_protocol::AutomationPage<crate::WakeReadResponse>, WakeClientError>
    {
        self.wakeup_call("wake_list", request).await
    }
    async fn wakeup_call<TRequest: Serialize, TResponse: DeserializeOwned>(
        &self,
        method: &str,
        request: TRequest,
    ) -> Result<TResponse, WakeClientError> {
        let params = serde_json::to_value(request)
            .map_err(|_| ClientError::Protocol("invalid wake request"))?;
        let shed = crate::admission_overload::ShedRequest::of(&params);
        let operation_id: Option<OperationId> = params
            .get("operationId")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| ClientError::Protocol("invalid Wake operation identity"))?;
        let wakeup_id: Option<collaboration_protocol::WakeupId> = params
            .get("wakeupId")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| ClientError::Protocol("invalid Wake identity"))?;
        let value = match self.connection.call(method, params).await {
            Ok(value) => value,
            Err(ClientError::Rejected {
                code: -32050,
                data: Some(data),
            }) => {
                if let Ok(error) =
                    serde_json::from_value::<crate::WakeSelectionFailure>(data.clone())
                {
                    if error.operation_id() != operation_id.as_ref()
                        || error.wakeup_id() != wakeup_id.as_ref()
                    {
                        return Err(ClientError::Protocol(
                            "Wake selection failure identity mismatch",
                        )
                        .into());
                    }
                    return Err(WakeClientError::Selection(Box::new(error)));
                }
                if let Ok(error) = serde_json::from_value::<CentralCatalogFailure>(data.clone()) {
                    if error.operation_id() != operation_id.as_ref() {
                        return Err(ClientError::Protocol(
                            "catalog Wake failure operation identity mismatch",
                        )
                        .into());
                    }
                    return Err(WakeClientError::Catalog(error));
                }
                let failure = serde_json::from_value(data)
                    .map_err(|_| ClientError::Protocol("invalid wake failure"))?;
                return Err(WakeClientError::Rejected(failure));
            }
            Err(ClientError::Overloaded { message }) => {
                return Err(WakeClientError::Rejected(
                    crate::admission_overload::wake(message, &shed).into(),
                ));
            }
            Err(error) => return Err(error.into()),
        };
        serde_json::from_value(value)
            .map_err(|_| ClientError::Protocol("invalid wake result").into())
    }
}
