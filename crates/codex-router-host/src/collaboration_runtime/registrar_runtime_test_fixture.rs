//! Isolated runtime and event-only capabilities; no fleet-store or restart proof.
use super::super::{CollaborationRuntime, CollaborationRuntimeInputs};
use crate::{ExternalProviderStartup, RuntimeTestRegistrarCapabilities};
use collaboration_service::fleet_session_registrar::{
    FleetSessionConnection, FleetSessionConnectionFailure, FleetSessionRequest,
    SessionBindingEntropy, SessionBindingEntropyUnavailable, SessionRegistrationMode,
    SessionRegistrationModeSource, test_support::MemoryRegistrationRepository,
};
use fleet_identity_protocol::{
    CheckSessionStart, EndFleetSession, MachineEnvironment, RegisterFleetSession,
    RegisterFleetSessionResult, SessionStartCheck, SessionStartRefusal,
};
use serde_json::{Value, json};
use std::{
    io,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

pub(super) const PROJECT: &str = "019f0000-0000-7000-8000-000000000002";
const WORKSPACE: &str = "019f0000-0000-7000-8000-000000000003";
const ORGANIZATION: &str = "019f0000-0000-7000-8000-000000000004";

pub(super) struct SessionAuthority {
    pub reject: AtomicBool,
    pub checks: Mutex<Vec<CheckSessionStart>>,
    pub registrations: Mutex<Vec<RegisterFleetSession>>,
}
impl FleetSessionConnection for SessionAuthority {
    fn check_start<'a>(
        &'a self,
        request: &'a CheckSessionStart,
    ) -> FleetSessionRequest<'a, SessionStartCheck> {
        Box::pin(async move {
            self.checks
                .lock()
                .map_err(|_| FleetSessionConnectionFailure::Unavailable)?
                .push(request.clone());
            if self.reject.load(Ordering::SeqCst) {
                Ok(SessionStartCheck::Refused {
                    reason: SessionStartRefusal::MachineNotAllowed {},
                })
            } else {
                Ok(SessionStartCheck::Allowed {
                    workspace_id: WORKSPACE
                        .to_owned()
                        .try_into()
                        .map_err(|_| FleetSessionConnectionFailure::MalformedReply)?,
                })
            }
        })
    }
    fn register<'a>(
        &'a self,
        request: &'a RegisterFleetSession,
    ) -> FleetSessionRequest<'a, RegisterFleetSessionResult> {
        Box::pin(async move {
            self.registrations
                .lock()
                .map_err(|_| FleetSessionConnectionFailure::Unavailable)?
                .push(request.clone());
            Ok(RegisterFleetSessionResult::Registered {
                organization_id: ORGANIZATION
                    .to_owned()
                    .try_into()
                    .map_err(|_| FleetSessionConnectionFailure::MalformedReply)?,
                workspace_id: WORKSPACE
                    .to_owned()
                    .try_into()
                    .map_err(|_| FleetSessionConnectionFailure::MalformedReply)?,
            })
        })
    }
    fn end<'a>(&'a self, _: &'a EndFleetSession) -> FleetSessionRequest<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
struct FixedMode(SessionRegistrationMode);
impl SessionRegistrationModeSource for FixedMode {
    fn mode(&self) -> SessionRegistrationMode {
        self.0
    }
}
struct LaunchIds(AtomicU64);
impl runtime_primitives::IdSource for LaunchIds {
    fn new_id(&self) -> uuid::Uuid {
        uuid::Uuid::from_u128(
            0x019f0000000070008000000000000100 + u128::from(self.0.fetch_add(1, Ordering::SeqCst)),
        )
    }
}
struct BindingEntropy(AtomicU64);
impl SessionBindingEntropy for BindingEntropy {
    fn new_capability(&self) -> Result<[u8; 16], SessionBindingEntropyUnavailable> {
        Ok((0x35353535353535353535353535353535_u128
            + u128::from(self.0.fetch_add(1, Ordering::SeqCst)))
        .to_be_bytes())
    }
}

pub(super) struct RuntimeFixture {
    pub root: tempfile::TempDir,
    pub repository: Arc<MemoryRegistrationRepository>,
    pub authority: Arc<SessionAuthority>,
}
impl RuntimeFixture {
    pub fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("service")).unwrap();
        std::fs::create_dir(root.path().join("codex-home")).unwrap();
        for directory in [root.path().join("service"), root.path().join("codex-home")] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self {
            root,
            repository: Arc::new(MemoryRegistrationRepository::new(Arc::new(Mutex::new(
                Vec::new(),
            )))),
            authority: Arc::new(SessionAuthority {
                reject: AtomicBool::new(false),
                checks: Mutex::new(Vec::new()),
                registrations: Mutex::new(Vec::new()),
            }),
        }
    }
    pub fn inputs(&self, fixed_port: bool) -> CollaborationRuntimeInputs {
        let mcp_bind: SocketAddr = if fixed_port {
            // This isolated in-process test configures a nonzero origin for
            // this one runtime. It makes no restart-reservation claim.
            let allocation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            allocation.local_addr().unwrap()
        } else {
            "127.0.0.1:0".parse().unwrap()
        };
        CollaborationRuntimeInputs {
            board_session: None,
            owner_human_id: Some("fixture-owner".to_owned().try_into().unwrap()),
            directory: self.root.path().join("service"),
            codex_home: self.root.path().join("codex-home"),
            backend_socket: self.root.path().join("backend.sock"),
            mcp_bind,
            native_schema: None,
            peer_registry_directory: Some(self.root.path().join("peers")),
            remote_control_server_name: None,
        }
    }
    pub fn capabilities(
        &self,
        mode: SessionRegistrationMode,
        repository: bool,
    ) -> RuntimeTestRegistrarCapabilities {
        RuntimeTestRegistrarCapabilities {
            repository: repository.then(|| self.repository.clone() as Arc<dyn codex_router_fleet_state::session_registration_repository::FleetSessionRegistrationRepository>),
            connection: self.authority.clone(),
            mode: Arc::new(FixedMode(mode)),
            confirmed_machine_environment: MachineEnvironment::CloudVm,
            ids: Arc::new(LaunchIds(AtomicU64::new(0))),
            entropy: Arc::new(BindingEntropy(AtomicU64::new(0))),
        }
    }
    pub async fn start(
        &self,
        fixed: bool,
        mode: SessionRegistrationMode,
        providers: Vec<ExternalProviderStartup>,
    ) -> io::Result<CollaborationRuntime> {
        CollaborationRuntime::runtime_test_start_with_registrar(
            self.inputs(fixed),
            providers,
            self.capabilities(mode, true),
        )
        .await
    }
    pub fn manifest(&self) -> collaboration_protocol::ServiceManifest {
        serde_json::from_slice(
            &std::fs::read(self.root.path().join("service/service.json")).unwrap(),
        )
        .unwrap()
    }
    pub fn create(&self, operation: &str, endpoint: &str, project: Option<&str>) -> Value {
        let mut input = json!({
            "operationId":operation,
            "endpoint":{"serviceId":self.manifest().service_id,"endpointId":endpoint},
            "workingDirectory":self.root.path(),
            "createdBy":{"kind":"human","humanId":"fixture-owner"},
            "approver":{"kind":"human","humanId":"fixture-owner"},
            "access":"workspace-write", "timeoutSeconds":2
        });
        if let Some(project) = project {
            input["projectId"] = json!(project);
        }
        input
    }
}

pub(super) async fn call(url: &str, tool: &str, arguments: Value) -> Value {
    let response = reqwest::Client::new().post(url)
        .header("Content-Type","application/json")
        .header("Accept","application/json, text/event-stream")
        .header("mcp-protocol-version","2025-11-25")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}}))
        .send().await.unwrap().text().await.unwrap();
    let payload = response
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap_or(&response);
    serde_json::from_str::<Value>(payload).unwrap()["result"].clone()
}
