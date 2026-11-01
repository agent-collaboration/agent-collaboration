//! Real Host adapter and registrar with deterministic authority and an event-controlled store stand-in.
use acp_client_runtime::{
    ProviderSessionCreateSetup, ProviderSessionMcp, RequestedProviderSettings,
};
use collaboration_contracts::{
    HarnessDeclaredLaunchInput, HarnessDeclaredSettings, HarnessMcpConfigurationIdentity,
    HarnessSessionLaunchPort, HarnessStartAdmission, PreparedHarnessSessionStart,
};
use collaboration_protocol::OperationId;
use collaboration_service::fleet_session_registrar::{
    FleetSessionConnection, FleetSessionRegistrar, FleetSessionRegistrarProps, FleetSessionRequest,
    SessionBindingEntropy, SessionBindingEntropyUnavailable, SessionBindingListener,
    SessionRegistrationMode, SessionRegistrationModeSource,
    test_support::MemoryRegistrationRepository,
};
use fleet_identity_protocol::{
    CheckSessionStart, EndFleetSession, MachineEnvironment, RegisterFleetSession,
    RegisterFleetSessionResult, SessionStartCheck,
};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const INSTALLATION: &str = "019f0000-0000-7000-8000-000000000001";
const PROJECT: &str = "019f0000-0000-7000-8000-000000000002";
const WORKSPACE: &str = "019f0000-0000-7000-8000-000000000003";
const ORGANIZATION: &str = "019f0000-0000-7000-8000-000000000004";

pub(super) struct Authority {
    pub registrations: Mutex<Vec<RegisterFleetSession>>,
}
impl FleetSessionConnection for Authority {
    fn check_start<'a>(
        &'a self,
        _: &'a CheckSessionStart,
    ) -> FleetSessionRequest<'a, SessionStartCheck> {
        Box::pin(async {
            Ok(SessionStartCheck::Allowed { workspace_id: WORKSPACE.to_owned().try_into().map_err(|_| collaboration_service::fleet_session_registrar::FleetSessionConnectionFailure::MalformedReply)? })
        })
    }
    fn register<'a>(
        &'a self,
        request: &'a RegisterFleetSession,
    ) -> FleetSessionRequest<'a, RegisterFleetSessionResult> {
        Box::pin(async move {
            self.registrations.lock().map_err(|_| collaboration_service::fleet_session_registrar::FleetSessionConnectionFailure::Unavailable)?.push(request.clone());
            Ok(RegisterFleetSessionResult::Registered {
                organization_id: ORGANIZATION.to_owned().try_into().map_err(|_| collaboration_service::fleet_session_registrar::FleetSessionConnectionFailure::MalformedReply)?,
                workspace_id: WORKSPACE.to_owned().try_into().map_err(|_| collaboration_service::fleet_session_registrar::FleetSessionConnectionFailure::MalformedReply)?,
            })
        })
    }
    fn end<'a>(&'a self, _: &'a EndFleetSession) -> FleetSessionRequest<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
struct Mode(SessionRegistrationMode);
impl SessionRegistrationModeSource for Mode {
    fn mode(&self) -> SessionRegistrationMode {
        self.0
    }
}
struct FixedIds(AtomicU64);
impl runtime_primitives::IdSource for FixedIds {
    fn new_id(&self) -> uuid::Uuid {
        uuid::Uuid::from_u128(
            0x019f0000000070008000000000000100 + u128::from(self.0.fetch_add(1, Ordering::SeqCst)),
        )
    }
}
struct Entropy;
impl SessionBindingEntropy for Entropy {
    fn new_capability(&self) -> Result<[u8; 16], SessionBindingEntropyUnavailable> {
        Ok([0x35; 16])
    }
}

pub(super) struct RegistrarFixture {
    pub registrar: Arc<FleetSessionRegistrar>,
    pub repository: Arc<MemoryRegistrationRepository>,
    pub authority: Arc<Authority>,
    port: Arc<dyn HarnessSessionLaunchPort>,
    pub operation: OperationId,
}
impl RegistrarFixture {
    pub fn new(mode: SessionRegistrationMode) -> Result<Self, Box<dyn std::error::Error>> {
        let repository = Arc::new(MemoryRegistrationRepository::new(Arc::new(Mutex::new(
            Vec::new(),
        ))));
        let authority = Arc::new(Authority {
            registrations: Mutex::new(Vec::new()),
        });
        let ids = Arc::new(FixedIds(AtomicU64::new(0)));
        let registrar = Arc::new(FleetSessionRegistrar::new(FleetSessionRegistrarProps {
            installation: INSTALLATION.to_owned().try_into()?,
            listener: SessionBindingListener::Stable(
                "http://127.0.0.1:43210".to_owned().try_into()?,
            ),
            repository: repository.clone(),
            connection: authority.clone(),
            mode: Arc::new(Mode(mode)),
            ids: ids.clone(),
            entropy: Arc::new(Entropy),
        }));
        let port = Arc::new(crate::HostHarnessSessionLaunchPort::new(
            registrar.clone(),
            MachineEnvironment::CloudVm,
            ids,
            crate::HostHarnessMcpServer::Provider,
        ));
        Ok(Self {
            registrar,
            repository,
            authority,
            port,
            operation: "019f0000-0000-7000-8000-000000000020"
                .to_owned()
                .try_into()?,
        })
    }
    pub async fn create_setup(
        &self,
        cwd: &Path,
        settings: &RequestedProviderSettings,
    ) -> Result<ProviderSessionCreateSetup, Box<dyn std::error::Error>> {
        let endpoint: collaboration_protocol::EndpointRef = serde_json::from_value(
            serde_json::json!({"serviceId":INSTALLATION,"endpointId":"claude-local"}),
        )?;
        let actor = collaboration_protocol::SessionRef {
            endpoint: endpoint.clone(),
            session_id: "permission-route-owner".to_owned().try_into()?,
        };
        let declared = HarnessDeclaredLaunchInput {
            project_id: Some(PROJECT.to_owned().try_into()?),
            endpoint: endpoint.clone(),
            working_directory: cwd
                .to_str()
                .ok_or("fixture cwd is not UTF-8")?
                .to_owned()
                .try_into()?,
            settings: HarnessDeclaredSettings::Provider {
                mode: settings.mode.clone().map(TryInto::try_into).transpose()?,
                model: settings.model.clone().map(TryInto::try_into).transpose()?,
                effort: settings.effort.clone().map(TryInto::try_into).transpose()?,
                access: collaboration_protocol::RouterAccess::WorkspaceWrite,
            },
            created_by: actor.clone().into(),
            approver: actor.into(),
            user_mcp: HarnessMcpConfigurationIdentity::empty(),
        };
        match self.port.prepare_start(&self.operation, &declared).await? {
            HarnessStartAdmission::AccessEnrollment => {
                Ok(ProviderSessionCreateSetup::AccessEnrollment {
                    mcp: ProviderSessionMcp::ConnectionDefaults,
                })
            }
            HarnessStartAdmission::Prepared(binding) => Ok(crate::prepare_provider_session_setup(
                PreparedHarnessSessionStart {
                    port: Arc::clone(&self.port),
                    operation_id: self.operation.clone(),
                    endpoint: message_board::SessionEndpointRef {
                        service_id: String::from(endpoint.service_id).try_into()?,
                        endpoint_id: String::from(endpoint.endpoint_id).try_into()?,
                    },
                    binding,
                },
            )
            .await?),
        }
    }
}
