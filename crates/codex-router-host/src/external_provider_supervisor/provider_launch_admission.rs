//! Provider create and restore select one retained session binding, never a fallback.
use super::{ExternalProviderSupervisor, ProviderSessionLaunchAdmission, failure};
use acp_client_runtime::{ProviderSessionCreateSetup, ProviderSessionMcp};
use collaboration_contracts::{
    HarnessDeclaredLaunchInput, HarnessDeclaredSettings, HarnessLaunchFailure,
    HarnessMcpConfigurationIdentity, HarnessRestoreAdmission, HarnessStartAdmission,
    PreparedHarnessSessionStart,
};
use collaboration_protocol::{
    ConversationCreateRequest, ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, EndpointRef, OperationId, ProviderOperationEffect,
    SessionRef,
};
use std::sync::Arc;

pub(super) enum ProviderCreateAdmission {
    AccessEnrollment,
    Managed(PreparedHarnessSessionStart),
}

impl ProviderCreateAdmission {
    pub(super) async fn prepare(
        admission: &ProviderSessionLaunchAdmission,
        request: &ConversationCreateRequest,
        supports_http: bool,
    ) -> Result<Self, ConversationOperationFailure> {
        let ProviderSessionLaunchAdmission::MachineTokens { port } = admission else {
            return Ok(Self::AccessEnrollment);
        };
        let prepare = async {
            let settings = request.settings.clone().unwrap_or_default();
            let declared = HarnessDeclaredLaunchInput {
                project_id: request.project_id.clone(),
                endpoint: request.endpoint.clone(),
                working_directory: String::from(request.working_directory.clone())
                    .try_into()
                    .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
                settings: HarnessDeclaredSettings::Provider {
                    mode: settings
                        .mode
                        .map(TryInto::try_into)
                        .transpose()
                        .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
                    model: settings
                        .model
                        .map(TryInto::try_into)
                        .transpose()
                        .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
                    effort: settings
                        .effort
                        .map(TryInto::try_into)
                        .transpose()
                        .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
                    access: request.requested_policy.access,
                },
                created_by: request.created_by.clone(),
                approver: request.approver.clone(),
                user_mcp: HarnessMcpConfigurationIdentity::empty(),
            };
            let endpoint = board_endpoint(&request.endpoint)?;
            let HarnessStartAdmission::Prepared(binding) =
                port.prepare_start(&request.operation_id, &declared).await?
            else {
                return Err(HarnessLaunchFailure::Unavailable);
            };
            Ok(Self::Managed(PreparedHarnessSessionStart {
                port: Arc::clone(port),
                operation_id: request.operation_id.clone(),
                endpoint,
                binding,
            }))
        }
        .await
        .map_err(|error| launch_failure(request.operation_id.clone(), None, error))?;
        if !supports_http {
            prepare
                .creation_refused()
                .await
                .map_err(|error| launch_failure(request.operation_id.clone(), None, error))?;
            return Err(failure(
                ConversationOperationFailureKind::UnsupportedCapability,
                ConversationOperationFailureStage::Binding,
                ProviderOperationEffect::None,
                "provider runtime does not support the required session HTTP MCP binding",
                request.operation_id.clone(),
                None,
            ));
        }
        Ok(prepare)
    }

    pub(super) async fn client_setup(
        &self,
    ) -> Result<ProviderSessionCreateSetup, HarnessLaunchFailure> {
        match self {
            Self::AccessEnrollment => Ok(ProviderSessionCreateSetup::AccessEnrollment {
                mcp: ProviderSessionMcp::ConnectionDefaults,
            }),
            Self::Managed(start) => crate::prepare_provider_session_setup(start.clone()).await,
        }
    }

    pub(super) async fn creation_refused(&self) -> Result<(), HarnessLaunchFailure> {
        match self {
            Self::AccessEnrollment => Ok(()),
            Self::Managed(start) => start.port.creation_refused(&start.operation_id).await,
        }
    }
}

impl ExternalProviderSupervisor {
    pub(crate) async fn session_mcp_for_restore(
        &self,
        target: &SessionRef,
    ) -> Result<ProviderSessionMcp, HarnessLaunchFailure> {
        let ProviderSessionLaunchAdmission::MachineTokens { port } = &self.inner.launch_admission
        else {
            return Ok(ProviderSessionMcp::ConnectionDefaults);
        };
        let session = message_board::SessionRef {
            endpoint: board_endpoint(&target.endpoint)?,
            session_id: String::from(target.session_id.clone())
                .try_into()
                .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
        };
        match port.binding_for_restore(&session).await? {
            HarnessRestoreAdmission::AccessEnrollment => Err(HarnessLaunchFailure::Unavailable),
            HarnessRestoreAdmission::Bound(binding) => Ok(ProviderSessionMcp::Http {
                server_name: binding.server_name().to_owned(),
                server_url: binding.expose_address().to_owned(),
            }),
        }
    }
}

fn board_endpoint(
    endpoint: &EndpointRef,
) -> Result<message_board::SessionEndpointRef, HarnessLaunchFailure> {
    Ok(message_board::SessionEndpointRef {
        service_id: String::from(endpoint.service_id.clone())
            .try_into()
            .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
        endpoint_id: String::from(endpoint.endpoint_id.clone())
            .try_into()
            .map_err(|_| HarnessLaunchFailure::InvalidInput)?,
    })
}

pub(super) fn launch_failure(
    operation: OperationId,
    target: Option<SessionRef>,
    error: HarnessLaunchFailure,
) -> ConversationOperationFailure {
    let (kind, explanation) = match error {
        HarnessLaunchFailure::ProjectRequired => (
            ConversationOperationFailureKind::ProjectRequired,
            "a session start must name its project",
        ),
        HarnessLaunchFailure::InvalidInput => (
            ConversationOperationFailureKind::InvalidRequest,
            "the declared provider launch input is invalid",
        ),
        HarnessLaunchFailure::AlreadyPrepared => (
            ConversationOperationFailureKind::Busy,
            "the retained launch must not create again",
        ),
        HarnessLaunchFailure::StartRefused(_) => (
            ConversationOperationFailureKind::PermissionRejected,
            "the organization authority refused the session start",
        ),
        HarnessLaunchFailure::InactiveBinding => (
            ConversationOperationFailureKind::PermissionRejected,
            "the retained session binding is inactive",
        ),
        HarnessLaunchFailure::Unavailable | HarnessLaunchFailure::InvalidTransition => (
            ConversationOperationFailureKind::Unavailable,
            "the session launch owner is unavailable",
        ),
    };
    failure(
        kind,
        ConversationOperationFailureStage::Admission,
        ProviderOperationEffect::None,
        explanation,
        operation,
        target,
    )
}
