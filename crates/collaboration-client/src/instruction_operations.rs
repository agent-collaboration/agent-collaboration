//! Typed instruction operations; callers retain operation identity across an uncertain connection.
use crate::{ClientError, CollaborationClient};
use collaboration_protocol::{
    InstructionCreateParams, InstructionFailure, InstructionFailureKind, InstructionShowParams,
    InstructionSnapshot, InstructionUpdateParams, OperationId,
};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum InstructionClientError {
    #[error(transparent)]
    Catalog(#[from] crate::central_catalog_failure::CentralCatalogFailure),
    #[error("instruction operation rejected: {0:?}")]
    Rejected(InstructionFailure),
    #[error(transparent)]
    Connection(#[from] ClientError),
}
impl CollaborationClient {
    pub async fn create_instruction(
        &self,
        params: InstructionCreateParams,
    ) -> Result<InstructionSnapshot, InstructionClientError> {
        let operation_id = params.operation_id.clone();
        self.instruction_call("instruction_create", params, Some(operation_id))
            .await
    }
    pub async fn update_instruction(
        &self,
        params: InstructionUpdateParams,
    ) -> Result<InstructionSnapshot, InstructionClientError> {
        let operation_id = params.operation_id.clone();
        self.instruction_call("instruction_update", params, Some(operation_id))
            .await
    }
    pub async fn read_instruction(
        &self,
        params: InstructionShowParams,
    ) -> Result<InstructionSnapshot, InstructionClientError> {
        self.instruction_call("instruction_show", params, None)
            .await
    }
    async fn instruction_call<TParams: Serialize>(
        &self,
        method: &str,
        params: TParams,
        operation_id: Option<OperationId>,
    ) -> Result<InstructionSnapshot, InstructionClientError> {
        let params = serde_json::to_value(params)
            .map_err(|_| ClientError::Protocol("invalid instruction request"))?;
        let shed = crate::admission_overload::ShedRequest::of(&params);
        let value = match self.connection.call(method, params).await {
            Ok(value) => value,
            Err(ClientError::Rejected {
                code: -32050,
                data: Some(data),
            }) => {
                // The API may shed a call before it reaches the catalog.
                if let Ok(failure) = serde_json::from_value::<InstructionFailure>(data.clone())
                    && matches!(failure.kind, InstructionFailureKind::Overloaded)
                {
                    return Err(InstructionClientError::Rejected(failure));
                }
                let failure: crate::central_catalog_failure::CentralCatalogFailure =
                    serde_json::from_value(data)
                        .map_err(|_| ClientError::Protocol("invalid central catalog failure"))?;
                if failure.operation_id() != operation_id.as_ref() {
                    return Err(ClientError::Protocol(
                        "central catalog failure operation identity mismatch",
                    )
                    .into());
                }
                return Err(InstructionClientError::Catalog(failure));
            }
            Err(ClientError::Overloaded { message }) => {
                return Err(InstructionClientError::Rejected(
                    crate::admission_overload::instruction(message, &shed),
                ));
            }
            Err(error) => return Err(error.into()),
        };
        serde_json::from_value(value)
            .map_err(|_| ClientError::Protocol("invalid instruction result").into())
    }
}
