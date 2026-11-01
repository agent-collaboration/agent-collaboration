//! Direct native callers consume the same Host launch owner as the client bridge.
use collaboration_contracts::{
    HarnessDeclaredLaunchInput, HarnessDeclaredSettings, HarnessForkInput, HarnessLaunchFailure,
    HarnessMcpConfigurationIdentity, HarnessNativePermissions, HarnessSessionLaunchPort,
    HarnessStartAdmission, PreparedHarnessSessionStart,
};
use collaboration_protocol::{DestinationPreparation, OperationId};
use std::sync::Arc;

#[derive(Clone, Default)]
pub enum NativeSessionRegistration {
    #[default]
    AccessEnrollment,
    MachineTokensUnavailable,
    MachineTokens {
        port: Arc<dyn HarnessSessionLaunchPort>,
    },
}

impl NativeSessionRegistration {
    pub(crate) async fn prepare_start(
        &self,
        operation: Option<&OperationId>,
        destination: &DestinationPreparation,
        model: Option<&str>,
        effort: &str,
    ) -> Result<Option<PreparedHarnessSessionStart>, HarnessLaunchFailure> {
        let port = match self {
            Self::AccessEnrollment => return Ok(None),
            Self::MachineTokensUnavailable => return Err(HarnessLaunchFailure::Unavailable),
            Self::MachineTokens { port } => port,
        };
        if matches!(destination, DestinationPreparation::Existing { .. }) {
            return Ok(None);
        }
        let operation = operation.ok_or(HarnessLaunchFailure::InvalidInput)?;
        let (endpoint, working_directory, project_id) = match destination {
            DestinationPreparation::Fresh {
                endpoint,
                cwd,
                project_id,
            } => (endpoint, cwd, project_id.clone()),
            DestinationPreparation::Fork {
                source,
                cwd,
                project_id,
                ..
            } => (&source.endpoint, cwd, project_id.clone()),
            DestinationPreparation::Existing { .. } => unreachable!(),
        };
        let fork = match destination {
            DestinationPreparation::Fork {
                source,
                through_turn_id,
                ..
            } => Some(HarnessForkInput {
                source: source.clone(),
                through_turn_id: Some(through_turn_id.clone()),
            }),
            _ => None,
        };
        let declared = HarnessDeclaredLaunchInput {
            endpoint: endpoint.clone(),
            project_id,
            working_directory: working_directory
                .to_owned()
                .try_into()
                .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
            settings: HarnessDeclaredSettings::Native {
                model: model
                    .map(|value| value.to_owned().try_into())
                    .transpose()
                    .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
                effort: (!effort.is_empty())
                    .then(|| effort.to_owned().try_into())
                    .transpose()
                    .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
                permissions: HarnessNativePermissions::InheritBackend,
                additional_directories: Vec::new(),
                fork,
                root_message_id: None,
            },
            user_mcp: HarnessMcpConfigurationIdentity::empty(),
        };
        let HarnessStartAdmission::Prepared(binding) =
            port.prepare_start(operation, &declared).await?
        else {
            return Err(HarnessLaunchFailure::Unavailable);
        };
        Ok(Some(PreparedHarnessSessionStart {
            port: port.clone(),
            operation_id: operation.clone(),
            endpoint: message_board::SessionEndpointRef {
                service_id: String::from(endpoint.service_id.clone())
                    .try_into()
                    .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
                endpoint_id: String::from(endpoint.endpoint_id.clone())
                    .try_into()
                    .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
            },
            binding,
        }))
    }
}
