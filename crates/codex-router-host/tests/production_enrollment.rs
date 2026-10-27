#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    clippy::large_futures,
    reason = "lint pass follows the crate split"
)]
//! Production Host startup discovers enrollment; only native/model traffic is a fixture.
#![deny(clippy::too_many_lines)]
use board_central_test_support::TestCentral;
use codex_router_host::{
    AppServerLaunchPlan, ChildCommandSpec, ChildOutput, HostConfig, HostConfigInputs,
    HostCoordinationPaths, HostDeadlines, HostRuntime, HostedReadiness, ManagedChildLaunchPlans,
    ManagedUpdateInputs, OperatorFrame, OperatorRequest,
};
use codex_router_test_support::router_health::PersistentRouterHealthFixture;
use collaboration_client::CollaborationClient;
use message_board::public_replies::{RequestDecision, RequestResult, WriteReply};
use std::{
    error::Error,
    io,
    net::SocketAddr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};

#[path = "support/production_enrollment_fixture.rs"]
mod enrollment_fixture;
#[path = "support/operator_client.rs"]
mod operator_client;
use enrollment_fixture::{EnrollmentLocation, ProductionEnrollmentFixture};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const BOUND: Duration = Duration::from_secs(20);
const ROOT_ENV: &str = "CODEX_PRODUCTION_ENROLLMENT_TEST_ROOT";
const ROUTER_ENV: &str = "CODEX_PRODUCTION_ENROLLMENT_TEST_ROUTER";
const CREDENTIAL_SENTINEL: &str = "Bearer enrollment-credential-must-stay-private";
const ACCESS_SENTINEL: &str = "enrollment-access-secret-must-stay-private";

#[tokio::test]

async fn production_host_discovers_enrollment_and_creates_a_central_project() -> TestResult {
    let fixture = ProductionEnrollmentFixture::start(EnrollmentLocation::RouterRoot).await?;
    fixture
        .run(async |fixture| {
            let directory = &fixture.directory;
            let peer = fixture.peer;
            let central = &fixture.central;
            let host = &fixture.host;
            host.await_ready().await?;
            let client = CollaborationClient::connect(directory, "enrollment-test", "1").await?;
            let (workspace_request, board_request, project_request) = project_requests()?;
            require_accepted(
                &settled_creation(|| client.workspace_create(workspace_request.clone())).await?,
            )?;
            require_accepted(
                &settled_creation(|| client.board_create(board_request.clone())).await?,
            )?;
            let project =
                settled_creation(|| client.project_create(project_request.clone())).await?;
            require_accepted(&project)?;
            let shown = client
                .project_show(message_board::ProjectShowRequest {
                    project_id: project_request.project_id.clone(),
                })
                .await?;
            require(
                shown.project.id == project_request.project_id,
                "created project was not read back",
            )?;
            require(
                matches!(shown.state, message_board::ProjectStateView::Local(_)),
                "created project is not local",
            )?;
            require(
                central.session_connections() > 0,
                "central did not accept a machine connection",
            )?;
            require(
                collaboration_runtime::load_installation_peer(directory)? == peer,
                "startup changed the persisted installation peer",
            )?;
            Ok::<_, Box<dyn Error>>(())
        })
        .await
}

#[tokio::test]

async fn production_host_without_enrollment_keeps_ready_and_creation_unreachable() -> TestResult {
    let fixture =
        ProductionEnrollmentFixture::start(EnrollmentLocation::CollaborationDirectory).await?;
    fixture
        .run(async |fixture| {
            let directory = &fixture.directory;
            let peer = fixture.peer;
            let central = &fixture.central;
            let host = &fixture.host;
            host.await_ready().await?;
            let client = CollaborationClient::connect(directory, "unenrolled-test", "1").await?;
            let (request, _, _) = project_requests()?;
            require(
                client.workspace_create(request).await?.result == RequestResult::PendingUnreachable,
                "unenrolled workspace creation changed its existing outcome",
            )?;
            require(
                central.session_connections() == 0,
                "Host read enrollment from the wrong root",
            )?;
            require(
                collaboration_runtime::load_installation_peer(directory)? == peer,
                "startup changed the persisted installation peer",
            )?;
            Ok::<_, Box<dyn Error>>(())
        })
        .await
}

#[tokio::test]
async fn production_host_refuses_malformed_enrollment_without_publishing() -> TestResult {
    let root = fixture_root()?;
    let path = write_private_enrollment(root.path())?;
    let mut enrollment: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    enrollment["expiresAt"] = serde_json::json!("not-a-timestamp");
    std::fs::write(path, serde_json::to_vec(&enrollment)?)?;
    require_startup_refusal(root.path(), "invalid fleet enrollment inputs").await
}

#[tokio::test]
async fn production_host_refuses_public_enrollment_without_publishing() -> TestResult {
    let root = fixture_root()?;
    let path = write_private_enrollment(root.path())?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))?;
    require_startup_refusal(root.path(), "private owner-only file").await
}

#[tokio::test]
async fn production_host_refuses_dangling_enrollment_link_instead_of_falling_back() -> TestResult {
    let root = fixture_root()?;
    let fleet = root.path().join("router/fleet");
    std::fs::create_dir(&fleet)?;
    std::os::unix::fs::symlink(fleet.join("absent.json"), fleet.join("enrollment.json"))?;
    require_startup_refusal(root.path(), "private owner-only file").await
}

#[tokio::test]
async fn production_host_preserves_the_installation_peer_mismatch_refusal() -> TestResult {
    let root = fixture_root()?;
    collaboration_runtime::load_installation_peer(&root.path().join("collaboration"))?;
    write_private_enrollment(root.path())?;
    require_startup_refusal(
        root.path(),
        "enrollment peer differs from this installation's persisted peer",
    )
    .await
}

fn write_private_enrollment(root: &Path) -> TestResult<PathBuf> {
    use codex_router_core::redaction::SecretString;
    use codex_router_secret_store::{
        SecretStore,
        fleet_secret_key::{FleetSecretKey, FleetSecretKind},
    };
    let router_root = root.join("router");
    let store = codex_router_host::open_fleet_enrollment_store(&router_root)?;
    let installation = "550e8400-e29b-41d4-a716-446655440000";
    let generation = uuid::Uuid::now_v7().to_string();
    let access = FleetSecretKey::new(
        FleetSecretKind::AccessClientSecret,
        installation,
        &generation,
    )?
    .secret_key()?;
    let credential = FleetSecretKey::new(
        FleetSecretKind::MachineCredential,
        installation,
        &generation,
    )?
    .secret_key()?;
    store.write_secret(&access, &SecretString::new(ACCESS_SENTINEL))?;
    store.write_secret(&credential, &SecretString::new(CREDENTIAL_SENTINEL))?;
    let document = codex_router_host::FleetEnrollmentDocument {
        endpoint: "ws://127.0.0.1:1/session".to_owned(),
        access: codex_router_host::FleetAccessReference {
            client_id: "fixture-access".to_owned(),
            client_secret_key: access.as_str().to_owned(),
        },
        credential_key: credential.as_str().to_owned(),
        peer: uuid::Uuid::now_v7().to_string().parse()?,
        receiver: "0192a1b2-c3d4-7000-8000-000000000002".parse()?,
        relay: "0192a1b2-c3d4-7000-8000-000000000003".parse()?,
        expires_at: None,
    };
    codex_router_host::persist_fleet_enrollment(&router_root, &document, &store)?;
    Ok(router_root.join("fleet/enrollment.json"))
}

async fn require_startup_refusal(root: &Path, expected: &str) -> TestResult {
    let router = PersistentRouterHealthFixture::start().await?;
    let mut host = OwnedHost::spawn(root, router.address())?;
    let outcome = tokio::time::timeout(BOUND, host.child.wait()).await;
    let stopped = host.stop().await;
    let router_stopped = router.finish().await;
    let children_gone = assert_owned_children_gone(root).await;
    let status = outcome??;
    require(!status.success(), "Host accepted invalid enrollment")?;
    stopped?;
    router_stopped?;
    children_gone?;
    let output = std::fs::read_to_string(root.join("host-output.log"))?;
    require(
        output.contains(expected),
        "startup error did not name the enrollment refusal",
    )?;
    require(
        !output.contains(CREDENTIAL_SENTINEL) && !output.contains(ACCESS_SENTINEL),
        "startup error exposed enrollment secrets",
    )?;
    for path in [
        "collaboration/service.json",
        "collaboration/control.sock",
        "router/operator.sock",
    ] {
        require(
            !root.join(path).exists(),
            "failed Host left a public collaboration endpoint",
        )?;
    }
    Ok(())
}

fn fixture_root() -> TestResult<tempfile::TempDir> {
    let root = tempfile::Builder::new().prefix("pe-").tempdir_in("/tmp")?;
    for name in ["router", "collaboration"] {
        let path = root.path().join(name);
        std::fs::create_dir(&path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    std::fs::write(root.path().join("native-pids.log"), b"")?;
    let executable = root.path().join("managed-codex");
    std::fs::write(
        &executable,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then\n echo 'codex-cli 1.2.3'\nelse\n exec \"$CODEX_ENROLLMENT_FIXTURE_BINARY\" \"$@\"\nfi\n",
    )?;
    std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700))?;
    Ok(root)
}

fn fixture_origin(directory: &Path) -> TestResult<message_board::ActorOrigin> {
    Ok(message_board::ActorOrigin {
        installation: String::from(collaboration_runtime::load_service_identity(directory)?)
            .try_into()?,
        machine: "production enrollment fixture".to_owned().try_into()?,
    })
}

fn require(condition: bool, message: &str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}

fn require_accepted(reply: &WriteReply) -> TestResult {
    if matches!(
        reply.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ) {
        Ok(())
    } else {
        Err(format!("Host did not complete central creation: {reply:?}").into())
    }
}

async fn settled_creation<Call, Reply>(mut call: Call) -> TestResult<WriteReply>
where
    Call: FnMut() -> Reply,
    Reply: std::future::Future<Output = Result<WriteReply, collaboration_client::BoardClientError>>,
{
    Ok(tokio::time::timeout(BOUND, async {
        loop {
            let reply = call().await?;
            if !matches!(
                reply.result,
                RequestResult::PendingUnreachable | RequestResult::ProjectCreationPending { .. }
            ) {
                return Ok::<_, collaboration_client::BoardClientError>(reply);
            }
            tokio::task::yield_now().await;
        }
    })
    .await??)
}

fn project_requests() -> TestResult<(
    message_board::WorkspaceCreateRequest,
    message_board::BoardCreateRequest,
    message_board::ProjectCreateRequest,
)> {
    use message_board::*;
    let actor = Identity::Human {
        human_id: "enrollment-test-owner".to_owned().try_into()?,
    };
    let workspace_id = WorkspaceId::generate();
    let board_id = BoardId::generate();
    Ok((
        WorkspaceCreateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            workspace_id: workspace_id.clone(),
            name: "Enrollment workspace".to_owned().try_into()?,
            description: String::new().try_into()?,
            actor: actor.clone(),
            acting_for: None,
        },
        BoardCreateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            board_id: board_id.clone(),
            workspace_id,
            name: "Enrollment board".to_owned().try_into()?,
            description: String::new().try_into()?,
            kind: BoardKind::Area {},
            actor: actor.clone(),
            acting_for: None,
        },
        ProjectCreateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            project_id: ProjectId::generate(),
            board_id,
            title: "Enrollment project".to_owned().try_into()?,
            description: None,
            actor,
            acting_for: None,
        },
    ))
}

fn write_enrollment(root: &Path, central: &TestCentral) -> TestResult {
    std::fs::create_dir_all(root.join("fleet"))?;
    let fixture = central.write_host_session(root).map_err(io::Error::other)?;
    std::fs::rename(fixture, root.join("fleet/enrollment.json"))?;
    Ok(())
}

struct OwnedHost {
    child: tokio::process::Child,
    operator: PathBuf,
}
impl OwnedHost {
    fn spawn(root: &Path, router: SocketAddr) -> TestResult<Self> {
        let output = std::fs::File::create(root.join("host-output.log"))?;
        let child = tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "production_enrollment_host_child",
                "--ignored",
                "--nocapture",
            ])
            .env(ROOT_ENV, root)
            .env(ROUTER_ENV, router.to_string())
            .stdout(output.try_clone()?)
            .stderr(output)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        Ok(Self {
            child,
            operator: root.join("router/operator.sock"),
        })
    }

    async fn await_ready(&self) -> TestResult {
        let frames = operator_client::send_operator_request(
            &self.operator,
            OperatorRequest::AwaitHostStart,
            BOUND,
        )
        .await?;
        let snapshot = frames
            .iter()
            .find_map(|frame| match frame {
                OperatorFrame::Terminal(response) => Some(response.snapshot()),
                OperatorFrame::Progress(_) => None,
            })
            .ok_or("Host startup did not publish a terminal snapshot")?;
        if snapshot.hosted_readiness() != HostedReadiness::Ready {
            return Err("Host did not reach Ready".into());
        }
        Ok(())
    }

    async fn stop(&mut self) -> TestResult<ExitStatus> {
        if let Some(status) = self.child.try_wait()? {
            return Ok(status);
        }
        let process_id = i32::try_from(self.child.id().ok_or("Host PID missing")?)?;
        let process_id = rustix::process::Pid::from_raw(process_id).ok_or("invalid Host PID")?;
        rustix::process::kill_process(process_id, rustix::process::Signal::TERM)?;
        Ok(tokio::time::timeout(BOUND, self.child.wait()).await??)
    }
}

async fn assert_owned_children_gone(root: &Path) -> TestResult {
    for process_id in std::fs::read_to_string(root.join("native-pids.log"))?.lines() {
        let process = tokio::process::Command::new("/bin/ps")
            .args(["-p", process_id, "-o", "pid="])
            .output()
            .await?;
        if !process.stdout.is_empty() {
            return Err(format!("owned native fixture {process_id} survived Host shutdown").into());
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "isolated production Host process, started by the enrollment tests"]
async fn production_enrollment_host_child() -> TestResult {
    let root = PathBuf::from(std::env::var_os(ROOT_ENV).ok_or("fixture root missing")?);
    let router = std::env::var(ROUTER_ENV)?.parse()?;
    let managed = root.join("managed-codex");
    let identity = codex_native_integration::executable_identity(&managed).await?;
    let native = root.join("native.sock");
    let command = ChildCommandSpec::new(managed.clone())
        .with_arguments([
            "--exact",
            "production_enrollment_native_child",
            "--ignored",
            "--nocapture",
        ])
        .with_environment("CODEX_ENROLLMENT_FIXTURE_BINARY", std::env::current_exe()?)
        .with_environment("CODEX_ENROLLMENT_NATIVE_SOCKET", &native)
        .with_environment("CODEX_ENROLLMENT_PROCESS_LOG", root.join("native-pids.log"))
        .with_output(ChildOutput::Null);
    let config = HostConfig::new(HostConfigInputs {
        coordination_paths: HostCoordinationPaths::new(
            root.join("router/operator.sock"),
            root.join("router/instance.lock"),
        ),
        router_endpoint: router,
        mcp_bind: ([127, 0, 0, 1], 0).into(),
        app_server_socket: native,
        managed_executable: managed,
        deadlines: HostDeadlines::production(),
    })
    .with_collaboration_directory(root.join("collaboration"), root.join("fixture-codex-home"))
    .with_collaboration_peer_registry_directory(root.join("no-peer-registry"))
    .with_owner_human_id("enrollment-test-owner".to_owned().try_into()?);
    HostRuntime::run(
        config,
        ManagedChildLaunchPlans::new(
            None,
            AppServerLaunchPlan::new(command, identity, "1.2.3".to_owned()),
        ),
        ManagedUpdateInputs::production(),
    )
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "native protocol fixture owned by the isolated enrollment Host"]
async fn production_enrollment_native_child() -> TestResult {
    let native =
        std::env::var_os("CODEX_ENROLLMENT_NATIVE_SOCKET").ok_or("native socket missing")?;
    let log = std::env::var_os("CODEX_ENROLLMENT_PROCESS_LOG").ok_or("process log missing")?;
    codex_router_test_support::native_app_server::run_native_app_server_fixture(
        Path::new(&native),
        "1.2.3",
        Some(Path::new(&log)),
    )
    .await
}
