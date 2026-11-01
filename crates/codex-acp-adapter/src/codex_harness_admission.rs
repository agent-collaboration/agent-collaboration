//! The Host chooses the enrollment bridge or its machine-token launch owner for this connection.
use collaboration_contracts::HarnessSessionLaunchPort;
use collaboration_protocol::EndpointRef;
use std::sync::Arc;

#[derive(Clone)]
pub enum CodexHarnessAdmission {
    AccessEnrollment,
    MachineTokens {
        endpoint: EndpointRef,
        port: Arc<dyn HarnessSessionLaunchPort>,
    },
}

impl CodexHarnessAdmission {
    pub(crate) async fn prepare_start(
        &self,
        catalog: &mut crate::AcpSchemaCatalog,
        params: &serde_json::Value,
        operation: &collaboration_protocol::OperationId,
    ) -> Result<crate::HarnessSessionSetup, crate::SessionSetupError> {
        let Self::MachineTokens { endpoint, port } = self else {
            return Ok(crate::HarnessSessionSetup::AccessEnrollment);
        };
        let declared = crate::native_declared_launch_input(catalog, params, endpoint)
            .map_err(|_| crate::SessionSetupError::InvalidParameters)?;
        match port
            .prepare_start(operation, &declared)
            .await
            .map_err(admission_failure)?
        {
            collaboration_contracts::HarnessStartAdmission::AccessEnrollment => {
                Ok(crate::HarnessSessionSetup::AccessEnrollment)
            }
            collaboration_contracts::HarnessStartAdmission::Prepared(binding) => Ok(
                crate::HarnessSessionSetup::Start(crate::PreparedHarnessSessionStart {
                    port: Arc::clone(port),
                    operation_id: operation.clone(),
                    endpoint: board_endpoint(endpoint)?,
                    binding,
                }),
            ),
        }
    }

    pub(crate) async fn restore(
        &self,
        params: &serde_json::Value,
    ) -> Result<crate::HarnessSessionSetup, crate::SessionSetupError> {
        let Self::MachineTokens { endpoint, port } = self else {
            return Ok(crate::HarnessSessionSetup::AccessEnrollment);
        };
        if params
            .pointer("/_meta/codexRouter")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|fields| fields.contains_key("projectId"))
        {
            return Err(crate::SessionSetupError::InvalidParameters);
        }
        let session = message_board::SessionRef {
            endpoint: board_endpoint(endpoint)?,
            session_id: params
                .get("sessionId")
                .and_then(serde_json::Value::as_str)
                .ok_or(crate::SessionSetupError::InvalidParameters)?
                .to_owned()
                .try_into()
                .map_err(|_| crate::SessionSetupError::InvalidParameters)?,
        };
        match port
            .binding_for_restore(&session)
            .await
            .map_err(admission_failure)?
        {
            collaboration_contracts::HarnessRestoreAdmission::AccessEnrollment => {
                Ok(crate::HarnessSessionSetup::AccessEnrollment)
            }
            collaboration_contracts::HarnessRestoreAdmission::Bound(binding) => {
                Ok(crate::HarnessSessionSetup::Restore(
                    crate::RestoredHarnessSession { session, binding },
                ))
            }
        }
    }
}

fn admission_failure(
    failure: collaboration_contracts::HarnessLaunchFailure,
) -> crate::SessionSetupError {
    match failure {
        collaboration_contracts::HarnessLaunchFailure::Unavailable => {
            crate::SessionSetupError::Unavailable
        }
        refusal => crate::SessionSetupError::LaunchAdmissionFailed(refusal),
    }
}

fn board_endpoint(
    endpoint: &EndpointRef,
) -> Result<message_board::SessionEndpointRef, crate::SessionSetupError> {
    Ok(message_board::SessionEndpointRef {
        service_id: String::from(endpoint.service_id.clone())
            .try_into()
            .map_err(|_| crate::SessionSetupError::InvalidParameters)?,
        endpoint_id: String::from(endpoint.endpoint_id.clone())
            .try_into()
            .map_err(|_| crate::SessionSetupError::InvalidParameters)?,
    })
}

#[cfg(test)]
mod admission_gate_tests;
