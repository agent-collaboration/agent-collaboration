//! An isolated production Host owns only these fixture child processes.
use codex_router_host::{
    AppServerLaunchPlan, ChildCommandSpec, ChildOutput, HostConfig, HostConfigInputs,
    HostCoordinationPaths, HostDeadlines, HostRuntime, HostedReadiness, ManagedChildLaunchPlans,
    ManagedUpdateInputs, OperatorFrame, OperatorRequest,
};
use std::{
    error::Error,
    io,
    net::SocketAddr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const BOUND: Duration = Duration::from_secs(20);
const ROOT_ENV: &str = "CODEX_FLEET_PRODUCER_HOST_ROOT";
const ROUTER_ENV: &str = "CODEX_FLEET_PRODUCER_ROUTER_ADDRESS";
pub fn fixture_root() -> TestResult<tempfile::TempDir> {
    let root = tempfile::Builder::new()
        .prefix("fleet-producer-debug-")
        .tempdir_in("/private/tmp")?;
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

pub async fn run_host_scenario<Observe>(root: &Path, observe: Observe) -> TestResult
where
    Observe: for<'host> std::ops::AsyncFnOnce(&'host OwnedHost) -> TestResult,
{
    let router =
        codex_router_test_support::router_health::PersistentRouterHealthFixture::start().await?;
    let host = match OwnedHost::spawn(root, router.address()) {
        Ok(host) => host,
        Err(error) => {
            let _stopped = router.finish().await;
            return Err(error);
        }
    };
    codex_router_test_support::scenario_finalization::run_scenario(
        (host, router),
        async |(host, _): &mut (
            OwnedHost,
            codex_router_test_support::router_health::PersistentRouterHealthFixture,
        )| {
            observe(host)
                .await
                .map_err(|error| io::Error::other(error.to_string()).into())
        },
        async |(mut host, router)| {
            let stopped = host.stop().await;
            let router_stopped = router.finish().await;
            let children_gone = assert_owned_children_gone(root).await;
            if !stopped
                .map_err(|error| io::Error::other(error.to_string()))?
                .success()
            {
                return Err(io::Error::other("Host shutdown failed").into());
            }
            router_stopped.map_err(|error| io::Error::other(error.to_string()))?;
            children_gone.map_err(|error| io::Error::other(error.to_string()))?;
            Ok(())
        },
        || Ok(()),
    )
    .await
    .map_err(io::Error::other)
    .map_err(Into::into)
}

pub struct OwnedHost {
    child: tokio::process::Child,
    operator: PathBuf,
}
impl OwnedHost {
    pub fn spawn(root: &Path, router: SocketAddr) -> TestResult<Self> {
        let output = std::fs::File::create(root.join("host-output.log"))?;
        let child = tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "fleet_host::host_child",
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

    pub async fn await_ready(&self) -> TestResult {
        let frames =
            send_operator_request(&self.operator, OperatorRequest::AwaitHostStart, BOUND).await?;
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

    pub async fn stop(&mut self) -> TestResult<ExitStatus> {
        if let Some(status) = self.child.try_wait()? {
            return Ok(status);
        }
        let process_id = i32::try_from(self.child.id().ok_or("Host PID missing")?)?;
        let process_id = rustix::process::Pid::from_raw(process_id).ok_or("invalid Host PID")?;
        rustix::process::kill_process(process_id, rustix::process::Signal::TERM)?;
        Ok(tokio::time::timeout(BOUND, self.child.wait()).await??)
    }
}

pub async fn assert_owned_children_gone(root: &Path) -> TestResult {
    let processes = std::fs::read_to_string(root.join("native-pids.log"))?;
    if processes.lines().next().is_none() {
        return Err("the ignored native fixture child never ran".into());
    }
    let host_output = std::fs::read_to_string(root.join("host-output.log"))?;
    if !host_output.contains("test result: ok. 1 passed") {
        return Err("the ignored production Host child did not finish its selected test".into());
    }
    for process_id in processes.lines() {
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
async fn host_child() -> TestResult {
    let root = PathBuf::from(std::env::var_os(ROOT_ENV).ok_or("fixture root missing")?);
    let router = std::env::var(ROUTER_ENV)?.parse()?;
    let managed = root.join("managed-codex");
    let identity = codex_native_integration::executable_identity(&managed).await?;
    let native = root.join("native.sock");
    let command = ChildCommandSpec::new(managed.clone())
        .with_arguments([
            "--exact",
            "fleet_host::native_child",
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
    .with_collaboration_directory(
        codex_router_host::router_collaboration_directory(&root.join("router")),
        root.join("fixture-codex-home"),
    )
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
async fn native_child() -> TestResult {
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
pub(crate) async fn send_operator_request(
    socket: &Path,
    request: OperatorRequest,
    deadline: Duration,
) -> Result<Vec<OperatorFrame>, Box<dyn Error>> {
    let deadline_at = tokio::time::Instant::now() + deadline;
    let mut stream = loop {
        match tokio::time::timeout_at(deadline_at, UnixStream::connect(socket)).await {
            Ok(Ok(stream)) => break stream,
            Ok(Err(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                if tokio::time::Instant::now() >= deadline_at {
                    return Err(std::io::Error::other("operator request timed out").into());
                }
                tokio::time::sleep_until(
                    deadline_at.min(tokio::time::Instant::now() + Duration::from_millis(20)),
                )
                .await;
            }
            Ok(Err(error)) => return Err(error.into()),
            Err(elapsed) => return Err(elapsed.into()),
        }
    };
    tokio::time::timeout_at(
        deadline_at,
        stream.write_all(&codex_router_host::encode_operator_request(&request)?),
    )
    .await??;
    tokio::time::timeout_at(deadline_at, stream.shutdown()).await??;
    let response_bytes = read_bounded_to_end(&mut stream, deadline_at).await?;

    let mut frames = Vec::new();
    let mut terminal_seen = false;
    for response_line in response_bytes.split_inclusive(|byte| *byte == b'\n') {
        if response_line.is_empty() {
            continue;
        }
        if terminal_seen {
            return Err(
                std::io::Error::other("operator response continued after terminal frame").into(),
            );
        }
        let frame = codex_router_host::decode_operator_frame(response_line)?;
        terminal_seen = matches!(frame, OperatorFrame::Terminal(_));
        frames.push(frame);
    }
    if !terminal_seen
        && frames
            .iter()
            .any(|frame| matches!(frame, OperatorFrame::Progress(_)))
    {
        return Ok(frames);
    }
    if !terminal_seen {
        return Err(std::io::Error::other("operator response omitted terminal frame").into());
    }
    Ok(frames)
}

async fn read_bounded_to_end(
    stream: &mut UnixStream,
    deadline_at: tokio::time::Instant,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut payload = Vec::new();
    let mut limited = stream
        .take(u64::try_from(codex_router_host::MAX_OPERATOR_FRAME_BYTES).unwrap_or(u64::MAX) + 1);
    tokio::time::timeout_at(deadline_at, limited.read_to_end(&mut payload)).await??;
    if payload.len() > codex_router_host::MAX_OPERATOR_FRAME_BYTES {
        return Err(std::io::Error::other("operator response exceeds protocol limit").into());
    }
    Ok(payload)
}
