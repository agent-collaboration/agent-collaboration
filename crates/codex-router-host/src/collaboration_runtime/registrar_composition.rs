//! One startup owner supplies the same registrar to MCP and both harness callers.
use std::{io, net::SocketAddr, sync::Arc};

use collaboration_protocol::{EndpointRef, UuidIdentity};
use collaboration_service::fleet_session_registrar::FleetSessionRegistrar;

#[cfg(feature = "runtime-test-support")]
use {
    codex_router_fleet_state::session_registration_repository::{
        FleetSessionRegistrationRepository, FleetSessionStorageFailure,
    },
    collaboration_service::fleet_session_registrar::{
        FleetSessionConnection, FleetSessionRegistrarProps, SessionBindingEntropy,
        SessionBindingListener, SessionRegistrationMode, SessionRegistrationModeSource,
    },
    fleet_identity_protocol::MachineEnvironment,
    runtime_primitives::IdSource,
};

/// Isolated capabilities; normal startup has no entry accepting this value.
#[cfg(feature = "runtime-test-support")]
pub struct RuntimeTestRegistrarCapabilities {
    pub repository: Option<Arc<dyn FleetSessionRegistrationRepository>>,
    pub connection: Arc<dyn FleetSessionConnection>,
    /// The deployment mode is fixed for this Runtime, independently of row presence.
    pub mode: Arc<dyn SessionRegistrationModeSource>,
    /// Stands in for the last confirmed F11 row; never inferred from this OS.
    pub confirmed_machine_environment: MachineEnvironment,
    pub ids: Arc<dyn IdSource + Send + Sync>,
    pub entropy: Arc<dyn SessionBindingEntropy>,
}

pub(super) enum RegistrarStartupInput {
    AccessEnrollment,
    #[cfg(feature = "runtime-test-support")]
    Injected(RuntimeTestRegistrarCapabilities),
}

pub(super) enum RegistrarRuntime {
    AccessEnrollment,
    #[cfg(feature = "runtime-test-support")]
    MachineTokens {
        registrar: Arc<FleetSessionRegistrar>,
        confirmed_machine_environment: MachineEnvironment,
        ids: Arc<dyn IdSource + Send + Sync>,
    },
}

impl RegistrarStartupInput {
    #[cfg(feature = "runtime-test-support")]
    fn injected(capabilities: RuntimeTestRegistrarCapabilities) -> io::Result<Self> {
        match capabilities.mode.mode() {
            SessionRegistrationMode::AccessEnrollment => Ok(Self::AccessEnrollment),
            SessionRegistrationMode::MachineTokens => {
                if capabilities.repository.is_none() {
                    return Err(io::Error::other(FleetSessionStorageFailure::Unavailable));
                }
                Ok(Self::Injected(capabilities))
            }
        }
    }

    pub(super) fn compose(
        self,
        service_id: &UuidIdentity,
        configured_mcp: SocketAddr,
        mcp_url: &str,
    ) -> io::Result<RegistrarRuntime> {
        match self {
            Self::AccessEnrollment => {
                // The existing path consumes none of the new-mode facts.
                let _ = (service_id, configured_mcp, mcp_url);
                Ok(RegistrarRuntime::AccessEnrollment)
            }
            #[cfg(feature = "runtime-test-support")]
            Self::Injected(capabilities) => {
                let repository = capabilities
                    .repository
                    .ok_or_else(|| io::Error::other(FleetSessionStorageFailure::Unavailable))?;
                let listener = if configured_mcp.port() == 0 {
                    SessionBindingListener::Ephemeral
                } else {
                    let origin = reqwest::Url::parse(mcp_url)
                        .map_err(io::Error::other)?
                        .origin()
                        .ascii_serialization()
                        .try_into()
                        .map_err(io::Error::other)?;
                    SessionBindingListener::Stable(origin)
                };
                Ok(RegistrarRuntime::MachineTokens {
                    registrar: Arc::new(FleetSessionRegistrar::new(FleetSessionRegistrarProps {
                        installation: String::from(service_id.clone())
                            .try_into()
                            .map_err(io::Error::other)?,
                        listener,
                        repository,
                        connection: capabilities.connection,
                        mode: capabilities.mode,
                        ids: capabilities.ids.clone(),
                        entropy: capabilities.entropy,
                    })),
                    confirmed_machine_environment: capabilities.confirmed_machine_environment,
                    ids: capabilities.ids,
                })
            }
        }
    }
}

impl RegistrarRuntime {
    pub(super) fn direct_native_registration(
        &self,
    ) -> collaboration_runtime::NativeSessionRegistration {
        match self {
            Self::AccessEnrollment => {
                collaboration_runtime::NativeSessionRegistration::AccessEnrollment
            }
            #[cfg(feature = "runtime-test-support")]
            Self::MachineTokens {
                registrar,
                confirmed_machine_environment,
                ids,
            } => collaboration_runtime::NativeSessionRegistration::MachineTokens {
                port: Self::launch_port(
                    registrar,
                    confirmed_machine_environment,
                    ids,
                    crate::HostHarnessMcpServer::Native,
                ),
            },
        }
    }

    #[cfg(feature = "runtime-test-support")]
    fn launch_port(
        registrar: &Arc<FleetSessionRegistrar>,
        confirmed_machine_environment: &MachineEnvironment,
        ids: &Arc<dyn IdSource + Send + Sync>,
        server: crate::HostHarnessMcpServer,
    ) -> Arc<dyn collaboration_contracts::HarnessSessionLaunchPort> {
        Arc::new(crate::HostHarnessSessionLaunchPort::new(
            registrar.clone(),
            confirmed_machine_environment.clone(),
            ids.clone(),
            server,
        ))
    }

    pub(super) fn provider_admission(&self) -> crate::ProviderSessionLaunchAdmission {
        match self {
            Self::AccessEnrollment => crate::ProviderSessionLaunchAdmission::AccessEnrollment,
            #[cfg(feature = "runtime-test-support")]
            Self::MachineTokens {
                registrar,
                confirmed_machine_environment,
                ids,
            } => crate::ProviderSessionLaunchAdmission::MachineTokens {
                port: Self::launch_port(
                    registrar,
                    confirmed_machine_environment,
                    ids,
                    crate::HostHarnessMcpServer::Provider,
                ),
            },
        }
    }

    pub(super) fn native_admission(
        &self,
        endpoint: &EndpointRef,
    ) -> codex_acp_adapter::CodexHarnessAdmission {
        match self {
            Self::AccessEnrollment => {
                let _ = endpoint;
                codex_acp_adapter::CodexHarnessAdmission::AccessEnrollment
            }
            #[cfg(feature = "runtime-test-support")]
            Self::MachineTokens {
                registrar,
                confirmed_machine_environment,
                ids,
            } => codex_acp_adapter::CodexHarnessAdmission::MachineTokens {
                endpoint: endpoint.clone(),
                port: Self::launch_port(
                    registrar,
                    confirmed_machine_environment,
                    ids,
                    crate::HostHarnessMcpServer::Native,
                ),
            },
        }
    }

    pub(super) fn registrar(&self) -> Option<Arc<FleetSessionRegistrar>> {
        match self {
            Self::AccessEnrollment => None,
            #[cfg(feature = "runtime-test-support")]
            Self::MachineTokens { registrar, .. } => Some(registrar.clone()),
        }
    }

    pub(super) fn bound_preparation(
        &self,
    ) -> Option<Arc<dyn collaboration_mcp::BoundConversationPreparation>> {
        match self {
            Self::AccessEnrollment => None,
            #[cfg(feature = "runtime-test-support")]
            Self::MachineTokens {
                registrar,
                confirmed_machine_environment,
                ids,
            } => Some(Arc::new(crate::HostBoundConversationPreparation::new(
                registrar.clone(),
                confirmed_machine_environment.clone(),
                ids.clone(),
            ))),
        }
    }
}

#[cfg(feature = "runtime-test-support")]
impl super::CollaborationRuntime {
    pub async fn runtime_test_start_with_registrar(
        inputs: super::CollaborationRuntimeInputs,
        providers: Vec<crate::ExternalProviderStartup>,
        capabilities: RuntimeTestRegistrarCapabilities,
    ) -> io::Result<Self> {
        let startup = RegistrarStartupInput::injected(capabilities)?;
        let (_sender, relation) =
            tokio::sync::watch::channel(collaboration_protocol::RouterExecutableRelation::Match);
        Box::pin(Self::start_with_optional_router_proxy_endpoint(
            inputs, providers, relation, None, None, startup,
        ))
        .await
    }
}
