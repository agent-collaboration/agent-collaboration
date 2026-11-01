//! F11-only process fixture: readiness is a socket event and finalization is child exit.
use std::{
    error::Error,
    io,
    net::SocketAddr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};

use codex_router_host::{
    AppServerLaunchPlan, ChildCommandSpec, ChildOutput, HostConfig, HostConfigInputs,
    HostCoordinationPaths, HostDeadlines, HostInstance, HostRuntime, HostedReadiness,
    ManagedChildLaunchPlans, ManagedUpdateInputs, OperatorFrame, OperatorRequest,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const ROOT_ENV: &str = "CODEX_F11_FIXTURE_ROOT";
const ROUTER_ENV: &str = "CODEX_F11_FIXTURE_ROUTER";
const BOUND_EVENT: &[u8] = b"operator-listener-bound";

struct PollingClock(AtomicI64);
impl codex_router_host::fleet_machine_login::FleetLoginClock for PollingClock {
    fn now(&self) -> fleet_sync_protocol::SyncTime {
        fleet_sync_protocol::SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
    fn wait_until(
        &self,
        deadline: fleet_sync_protocol::SyncTime,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ()>> + Send + '_>> {
        Box::pin(async move {
            if deadline.unix_millis() <= 5_000 {
                self.0.store(deadline.unix_millis(), Ordering::SeqCst);
                Ok(())
            } else {
                std::future::pending().await
            }
        })
    }
}

pub fn fixture_root() -> TestResult<tempfile::TempDir> {
    let root = tempfile::Builder::new()
        .prefix("f11-machine-login-")
        .tempdir_in("/tmp")?;
    let router = root.path().join("router");
    std::fs::create_dir(&router)?;
    std::fs::set_permissions(router, std::fs::Permissions::from_mode(0o700))?;
    std::fs::write(root.path().join("native-pids.log"), b"")?;
    let managed = root.path().join("managed-codex");
    std::fs::write(
        &managed,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then\n echo 'codex-cli 1.2.3'\nelse\n exec \"$CODEX_F11_NATIVE_BINARY\" \"$@\"\nfi\n",
    )?;
    std::fs::set_permissions(managed, std::fs::Permissions::from_mode(0o700))?;
    Ok(root)
}

struct ListenerEvent {
    listener: UnixListener,
    path: PathBuf,
}
impl ListenerEvent {
    fn bind(root: &Path) -> TestResult<Self> {
        let path = root.join("fixture-bound.sock");
        Ok(Self {
            listener: UnixListener::bind(&path)?,
            path,
        })
    }
    async fn wait(&self) -> TestResult {
        let (mut stream, _) = self.listener.accept().await?;
        let mut bytes = Vec::new();
        (&mut stream).take(64).read_to_end(&mut bytes).await?;
        if bytes != BOUND_EVENT {
            return Err("Host fixture sent an invalid readiness event".into());
        }
        Ok(())
    }
}
impl Drop for ListenerEvent {
    fn drop(&mut self) {
        let _removed = std::fs::remove_file(&self.path);
    }
}

pub struct OwnedHost {
    child: tokio::sync::Mutex<tokio::process::Child>,
    operator: PathBuf,
    bound: ListenerEvent,
}
impl OwnedHost {
    fn spawn(root: &Path, router: SocketAddr) -> TestResult<Self> {
        let bound = ListenerEvent::bind(root)?;
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
            .env_remove("CODEX_FIXTURE_IGNORE_TERM")
            .stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(output)
            .kill_on_drop(true)
            .spawn()?;
        Ok(Self {
            child: tokio::sync::Mutex::new(child),
            operator: HostCoordinationPaths::for_router_root(&root.join("router"))
                .operator_socket()
                .to_owned(),
            bound,
        })
    }
    pub async fn await_ready(&self) -> TestResult {
        let mut child = self.child.lock().await;
        tokio::select! {
            bound = self.bound.wait() => bound?,
            status = child.wait() => return Err(io::Error::other(format!("Host child exited before listener readiness: {}", status?)).into()),
        }
        drop(child);
        let mut stream = UnixStream::connect(&self.operator).await?;
        stream
            .write_all(&codex_router_host::encode_operator_request(
                &OperatorRequest::AwaitHostStart,
            )?)
            .await?;
        stream.shutdown().await?;
        let mut bytes = Vec::new();
        (&mut stream)
            .take(u64::try_from(codex_router_host::MAX_OPERATOR_FRAME_BYTES)? + 1)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > codex_router_host::MAX_OPERATOR_FRAME_BYTES {
            return Err("oversized Host startup frame".into());
        }
        let mut terminal = None;
        for line in bytes.split_inclusive(|byte| *byte == b'\n') {
            let frame = codex_router_host::decode_operator_frame(line)?;
            if terminal.is_some() {
                return Err("Host startup continued after its terminal".into());
            }
            match frame {
                OperatorFrame::Terminal(response) => terminal = Some(response),
                OperatorFrame::Progress(_) => {}
                OperatorFrame::FleetLogin(_) => {
                    return Err("unexpected login frame during startup".into());
                }
            }
        }
        if terminal
            .as_ref()
            .is_none_or(|response| response.snapshot().hosted_readiness() != HostedReadiness::Ready)
        {
            return Err("Host did not publish its Ready terminal".into());
        }
        Ok(())
    }
    async fn stop(&mut self) -> TestResult<ExitStatus> {
        let mut child = self.child.lock().await;
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        let process = rustix::process::Pid::from_raw(i32::try_from(
            child.id().ok_or("Host child PID missing")?,
        )?)
        .ok_or("invalid Host child PID")?;
        rustix::process::kill_process(process, rustix::process::Signal::TERM)?;
        Ok(child.wait().await?)
    }
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
            let _closed = router.finish().await;
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
            let gone = assert_owned_children_gone(root).await;
            if !stopped
                .map_err(|error| io::Error::other(error.to_string()))?
                .success()
            {
                return Err(io::Error::other("Host child shutdown failed").into());
            }
            router_stopped.map_err(|error| io::Error::other(error.to_string()))?;
            gone.map_err(|error| io::Error::other(error.to_string()))?;
            Ok(())
        },
        || Ok(()),
    )
    .await
    .map_err(io::Error::other)
    .map_err(Into::into)
}

async fn assert_owned_children_gone(root: &Path) -> TestResult {
    let processes = std::fs::read_to_string(root.join("native-pids.log"))?;
    if processes.lines().next().is_none() {
        return Err("native fixture child never ran".into());
    }
    let output = std::fs::read_to_string(root.join("host-output.log"))?;
    if !output.contains("test result: ok. 1 passed") {
        return Err("Host child did not complete its selected test".into());
    }
    for process in processes.lines() {
        let result = tokio::process::Command::new("/bin/ps")
            .args(["-p", process, "-o", "pid="])
            .output()
            .await?;
        if !result.stdout.is_empty() {
            return Err(format!("owned native fixture {process} survived shutdown").into());
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "F11's isolated Host child, started by the actual CLI journey"]
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
        .with_environment("CODEX_F11_NATIVE_BINARY", std::env::current_exe()?)
        .with_environment("CODEX_F11_NATIVE_SOCKET", &native)
        .with_environment("CODEX_F11_PROCESS_LOG", root.join("native-pids.log"))
        .with_output(ChildOutput::Null);
    let config = HostConfig::new(HostConfigInputs {
        coordination_paths: HostCoordinationPaths::for_router_root(&root.join("router")),
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
    .with_owner_human_id("f11-test-owner".to_owned().try_into()?);
    let instance = HostInstance::acquire(config.coordination_paths().clone())?;
    let mut ready = UnixStream::connect(root.join("fixture-bound.sock")).await?;
    ready.write_all(BOUND_EVENT).await?;
    ready.shutdown().await?;
    Box::pin(HostRuntime::run_acquired_with_fleet_login_dependencies(
        config,
        ManagedChildLaunchPlans::new(
            None,
            AppServerLaunchPlan::new(command, identity, "1.2.3".to_owned()),
        ),
        ManagedUpdateInputs::production(),
        instance,
        codex_router_host::fleet_machine_login::FleetLoginDependencies {
            clock: Arc::new(PollingClock(AtomicI64::new(0))),
            keychain: Arc::new(
                codex_router_secret_store::test_support::DeterministicTestKeychainAccess,
            ),
        },
    ))
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "F11 native protocol child, owned by its isolated Host"]
async fn native_child() -> TestResult {
    let native = std::env::var_os("CODEX_F11_NATIVE_SOCKET").ok_or("native socket missing")?;
    let log = std::env::var_os("CODEX_F11_PROCESS_LOG").ok_or("native process log missing")?;
    codex_router_test_support::native_app_server::run_native_app_server_fixture(
        Path::new(&native),
        "1.2.3",
        Some(Path::new(&log)),
    )
    .await
}
