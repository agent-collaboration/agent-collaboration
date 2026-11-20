//! Owns and joins one debug Host child for listener startup proof.
use codex_router_test_support::FixtureResult;
use owned_process_test_support::owned_cli_process::run_cli_output;
use serde::Deserialize;
use std::{
    fs::OpenOptions,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, ChildStdout, Command},
};

const READY_BOUND: Duration = Duration::from_secs(45);
const STOP_BOUND: Duration = Duration::from_secs(20);
const MAX_START_ATTEMPTS: usize = 3;

enum ReadyListener {
    Accepted,
    ProtectedPort(u16),
}

#[must_use]
pub(super) fn protected_listener_port(actual_port: u16, reported_port: u16) -> Option<u16> {
    [actual_port, reported_port]
        .into_iter()
        .find(|port| matches!(*port, 8787 | 8788 | 19741 | 19742 | 19743 | 43127))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ready {
    kind: String,
    directory: PathBuf,
    mcp_url: String,
    native_backend: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BeforeServeHeld {
    kind: String,
    directory: PathBuf,
}

pub struct BoardHost {
    pub directory: PathBuf,
    pub mcp_url: String,
    child: Child,
    output: BufReader<ChildStdout>,
    hold_before_serve: bool,
}

/// Evidence that this exact owned child was joined before native fixture readback.
pub struct StoppedHost {
    directory: PathBuf,
}
impl StoppedHost {
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    // SIGKILL skips listener Drop; only this joined child's three paths qualify.
    // Shared by targets that exercise joined SIGKILL readback.
}

pub fn artifact_command(binary: &Path) -> FixtureResult<Command> {
    if !binary.is_absolute() || !binary.is_file() {
        return Err(format!(
            "prebuilt absolute test artifact is missing: {}",
            binary.display()
        )
        .into());
    }
    #[cfg(target_os = "macos")]
    {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut command = Command::new(root.join("scripts/cargo_debug_signing_runner.sh"));
        command.arg(binary);
        Ok(command)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(Command::new(binary))
    }
}

impl BoardHost {
    pub async fn start(directory: &Path) -> FixtureResult<Self> {
        Self::start_with_port_reports(directory, std::convert::identity)
            .await
            .map(|(host, _)| host)
    }

    /// Port reports can force rejection, but cannot permit a protected actual listener.
    pub async fn start_with_port_reports(
        directory: &Path,
        mut report_port: impl FnMut(u16) -> u16,
    ) -> FixtureResult<(Self, Vec<StoppedHost>)> {
        let mut rejected_ports = Vec::new();
        let mut stopped_attempts = Vec::new();
        for _ in 0..MAX_START_ATTEMPTS {
            let mut host = Self::spawn_owned(directory)?;
            match host.read_ready(&mut report_port).await {
                Ok(ReadyListener::Accepted) => return Ok((host, stopped_attempts)),
                Ok(ReadyListener::ProtectedPort(port)) => {
                    // Port 0 can select a protected port on Linux. Join this
                    // exact child before asking the OS for a fresh listener.
                    stopped_attempts.push(host.stop_for_readback().await.map_err(|error| {
                        format!("Host on protected listener port {port} failed to stop: {error}")
                    })?);
                    rejected_ports.push(port);
                }
                Err(error) => {
                    return match host.stop().await {
                        Ok(()) => Err(error),
                        Err(cleanup) => Err(format!(
                            "Host readiness failed: {error}; cleanup failed: {cleanup}"
                        )
                        .into()),
                    };
                }
            }
        }
        Err(format!(
            "Host reported protected listener ports {rejected_ports:?} after {MAX_START_ATTEMPTS} attempts; every rejected child was stopped"
        )
        .into())
    }

    fn spawn_owned(directory: &Path) -> FixtureResult<Self> {
        let binary = std::env::var_os("CODEX_ROUTER_E2E_HOST_BINARY")
            .map(PathBuf::from)
            .ok_or(
                "set CODEX_ROUTER_E2E_HOST_BINARY to prebuilt target/debug/board-debug-runtime",
            )?;
        let stderr = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join(format!("host-{}.stderr", uuid::Uuid::now_v7())))?;
        let mut command = artifact_command(&binary)?;
        command.arg(directory).arg("0");
        command
            .env_remove("CODEX_ROUTER_HOST_INHERITED_LOCK")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let output = child.stdout.take().ok_or("owned Host stdout is absent")?;
        Ok(Self {
            directory: directory.to_owned(),
            mcp_url: String::new(),
            child,
            output: BufReader::new(output),
            hold_before_serve: directory.join("fixture-hold-before-serve").is_file(),
        })
    }

    async fn read_ready(
        &mut self,
        report_port: &mut impl FnMut(u16) -> u16,
    ) -> FixtureResult<ReadyListener> {
        let mut line = String::new();
        if tokio::time::timeout(READY_BOUND, self.output.read_line(&mut line)).await?? == 0 {
            return Err("owned Host exited before its ready record".into());
        }
        let ready: Ready = serde_json::from_str(&line)?;
        if ready.kind != "boardDebugRuntimeReady"
            || ready.native_backend != "absent"
            || ready.directory.canonicalize()? != self.directory.canonicalize()?
        {
            return Err("ready record names a different Host fixture".into());
        }
        let manifest: collaboration_client::protocol::ServiceManifest =
            serde_json::from_slice(&std::fs::read(self.directory.join("service.json"))?)?;
        let url = reqwest::Url::parse(&ready.mcp_url)?;
        if ready.mcp_url != manifest.mcp.url
            || url.host_str() != Some("127.0.0.1")
            || url.scheme() != "http"
            || url.port().is_none()
        {
            return Err(format!(
                "Host ready/manifest/listener identity mismatch: same_url={}, http={}, loopback={}, port={:?}",
                ready.mcp_url == manifest.mcp.url,
                url.scheme() == "http",
                url.host_str() == Some("127.0.0.1"),
                url.port()
            )
            .into());
        }
        if self.hold_before_serve {
            self.wait_before_serve_hold().await?;
        }
        let actual_port = url.port().ok_or("owned Host listener port is absent")?;
        if let Some(port) = protected_listener_port(actual_port, report_port(actual_port)) {
            return Ok(ReadyListener::ProtectedPort(port));
        }
        self.mcp_url = ready.mcp_url;
        Ok(ReadyListener::Accepted)
    }

    async fn wait_before_serve_hold(&mut self) -> FixtureResult {
        let mut line = String::new();
        if tokio::time::timeout(READY_BOUND, self.output.read_line(&mut line)).await?? == 0 {
            return Err("owned Host exited before its pre-serve hold acknowledgement".into());
        }
        let held: BeforeServeHeld = serde_json::from_str(&line)?;
        if held.kind != "boardDebugRuntimeBeforeServeHeld"
            || held.directory.canonicalize()? != self.directory.canonicalize()?
        {
            return Err("pre-serve hold acknowledgement names a different Host".into());
        }
        Ok(())
    }

    pub async fn stop_for_readback(self) -> FixtureResult<StoppedHost> {
        let directory = self.directory.canonicalize()?;
        self.stop().await?;
        Ok(StoppedHost { directory })
    }

    pub async fn stop(mut self) -> FixtureResult {
        // Release the debug example's stdin reader before joining its runtime.
        drop(self.child.stdin.take());
        if self.child.try_wait()?.is_none() {
            let pid = self.child.id().ok_or("owned Host PID absent before stop")?;
            let mut signal = Command::new("kill");
            signal.args(["-TERM", &pid.to_string()]);
            let signaled = run_cli_output(signal, Duration::from_secs(5)).await?;
            if !signaled.status.success() {
                return Err("signal to the exact owned Host failed".into());
            }
        }
        let mut remaining = Vec::new();
        let settled = tokio::time::timeout(STOP_BOUND, async {
            let (status, _) =
                tokio::try_join!(self.child.wait(), self.output.read_to_end(&mut remaining))?;
            Ok::<_, std::io::Error>(status)
        })
        .await;
        match settled {
            Ok(Ok(status)) if status.success() => Ok(()),
            Ok(Ok(status)) => Err(format!("Host shutdown returned {status}").into()),
            Ok(Err(error)) => Err(error.into()),
            Err(error) => {
                let killed = self.child.start_kill();
                let waited = self.child.wait().await;
                Err(format!(
                    "Host shutdown timed out: {error}; fallback kill={killed:?}, wait={waited:?}"
                )
                .into())
            }
        }
    }
}
