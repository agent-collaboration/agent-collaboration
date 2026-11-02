//! One owner keeps both real enrolled Hosts and their central alive through explicit shutdown.
use crate::{CollaborationRuntime, CollaborationRuntimeInputs};
use board_central_test_support::{EnrolledTestMachine, TestCentral};
use collaboration_runtime::SessionAutomationTransport;
use message_board::ActorOrigin;
use std::{
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub(super) type ProofResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;

pub(super) struct CatalogHosts {
    pub first: CollaborationRuntime,
    pub second: CollaborationRuntime,
    pub first_root: PathBuf,
    pub second_root: PathBuf,
    machine: EnrolledTestMachine,
    central: TestCentral,
    directory: tempfile::TempDir,
}
impl CatalogHosts {
    pub async fn open() -> ProofResult<Self> {
        let directory = tempfile::Builder::new()
            .prefix("s6cat-")
            .tempdir_in("/tmp")?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        let first_root = directory.path().join("a");
        let second_root = directory.path().join("b");
        let first_origin = private_directory(&first_root)?;
        let second_origin = private_directory(&second_root)?;
        let first_peer = collaboration_runtime::load_installation_peer(&first_root)?;
        let second_peer = collaboration_runtime::load_installation_peer(&second_root)?;
        let central = TestCentral::start_with_peer(
            &directory.path().join("central"),
            &first_origin,
            first_peer,
        )
        .await?;
        let machine = central
            .additional_machine(&second_origin, second_peer)
            .await?;
        let first = match start_host(&first_root, central.session_inputs()?).await {
            Ok(host) => host,
            Err(error) => {
                let machine_stopped = machine.shutdown().await;
                let central_stopped = central.shutdown().await;
                machine_stopped?;
                central_stopped?;
                return Err(error.into());
            }
        };
        let second = match start_host(&second_root, machine.session_inputs()?).await {
            Ok(host) => host,
            Err(error) => {
                let first_stopped = first.shutdown().await;
                let machine_stopped = machine.shutdown().await;
                let central_stopped = central.shutdown().await;
                first_stopped?;
                machine_stopped?;
                central_stopped?;
                return Err(error.into());
            }
        };
        Ok(Self {
            first,
            second,
            first_root,
            second_root,
            machine,
            central,
            directory,
        })
    }
    pub async fn close(self) -> ProofResult {
        let first = self.first.shutdown().await;
        let second = self.second.shutdown().await;
        let machine = self.machine.shutdown().await;
        let central = self.central.shutdown().await;
        let removed = self.directory.close();
        first?;
        second?;
        machine?;
        central?;
        removed?;
        Ok(())
    }
}

pub(super) fn central_wake_created(
    response: collaboration_client::WakeCreateResponse,
) -> ProofResult<collaboration_protocol::CentralWakeSnapshot> {
    match response {
        collaboration_client::WakeCreateResponse::Central { value } => Ok(value),
        collaboration_client::WakeCreateResponse::Legacy { .. } => {
            Err("new operation returned old-operation recovery instead of a central Wake".into())
        }
    }
}

fn private_directory(root: &Path) -> ProofResult<ActorOrigin> {
    std::fs::DirBuilder::new().mode(0o700).create(root)?;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(root.join("peers"))?;
    Ok(ActorOrigin {
        installation: String::from(collaboration_runtime::load_service_identity(root)?)
            .try_into()?,
        machine: "central catalog Host fixture".to_owned().try_into()?,
    })
}
async fn start_host(
    root: &Path,
    enrollment: board_fleet_sync::BoardSessionInputs,
) -> std::io::Result<CollaborationRuntime> {
    CollaborationRuntime::start(CollaborationRuntimeInputs {
        directory: root.to_owned(),
        codex_home: root.to_owned(),
        backend_socket: root.join("absent-native.sock"),
        mcp_bind: ([127, 0, 0, 1], 0).into(),
        native_schema: None,
        peer_registry_directory: Some(root.join("peers")),
        remote_control_server_name: None,
        owner_human_id: None,
        board_session: Some(enrollment),
    })
    .await
}
pub(super) async fn connected_transport(
    runtime: &CollaborationRuntime,
) -> ProofResult<SessionAutomationTransport> {
    let projects = runtime
        .board_session
        .as_ref()
        .and_then(|session| session.projects.upgrade())
        .ok_or("Host machine-session owner missing")?;
    super::wait_welcome(projects.welcomed.clone()).await?;
    Ok(SessionAutomationTransport::new(projects.requests.clone()))
}

/// Choose valid future fixture data; no test waits for this deadline or submits native work.
pub(super) fn future_wake_time() -> ProofResult<collaboration_protocol::ObservationTimestamp> {
    let due = collaboration_protocol::GridTime::next_after(
        chrono::Utc::now().timestamp_millis() + 3_600_000,
    )?;
    collaboration_runtime::observation_timestamp::timestamp(due.unix_millis())
        .map_err(|()| "future fixture timestamp is out of range".into())
}

/// One actual pull establishes this fixture Host's machine-work capability, without polling.
pub(super) async fn machine_work_ready(transport: &SessionAutomationTransport) -> ProofResult {
    use collaboration_protocol::{
        AutomationAnswer, AutomationCall, MachineWorkProtocol, PullAutomationBatches,
    };
    use collaboration_runtime::CentralAutomationTransport;
    match transport
        .call(AutomationCall::PullAutomationBatches(
            PullAutomationBatches {
                after: None,
                max_bytes: 1_048_576,
                work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
            },
        ))
        .await?
    {
        AutomationAnswer::Batches(_) => Ok(()),
        other => Err(format!("fixture machine-work capability was not admitted: {other:?}").into()),
    }
}
