//! Owned process and public listener access for the real board Host fixture.
use codex_router_test_support::{FixtureResult, owned_cli_process::run_cli_output};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    fs::OpenOptions,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdout, Command},
};

const READY_BOUND: Duration = Duration::from_secs(45);
const STOP_BOUND: Duration = Duration::from_secs(20);
const CALL_BOUND: Duration = Duration::from_secs(20);
const MAX_START_ATTEMPTS: usize = 3;

enum ReadyListener {
    Accepted,
    ProtectedPort(u16),
}

#[must_use]
pub(super) fn protected_listener_port(actual_port: u16, reported_port: u16) -> Option<u16> {
    [actual_port, reported_port]
        .into_iter()
        .find(|port| matches!(*port, 8787 | 8788 | 43127))
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
    async fn remove_listener_residue(&self) -> FixtureResult {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};

        let mut failures = Vec::new();
        for name in ["control.sock", "codex-native.sock", "codex-acp.sock"] {
            let path = self.directory.join(name);
            let removed: FixtureResult = async {
                let before = match std::fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                    Err(error) => return Err(error.into()),
                };
                if !before.file_type().is_socket() {
                    return Err("joined Host listener path is not a socket".into());
                }
                match tokio::time::timeout(
                    Duration::from_secs(2),
                    tokio::net::UnixStream::connect(&path),
                )
                .await?
                {
                    Ok(_) => return Err("joined Host socket still has an active listener".into()),
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                    Err(error) => return Err(error.into()),
                }
                let after = std::fs::symlink_metadata(&path)?;
                if !after.file_type().is_socket()
                    || before.dev() != after.dev()
                    || before.ino() != after.ino()
                {
                    return Err("joined Host listener inode changed during cleanup".into());
                }
                std::fs::remove_file(&path)?;
                Ok(())
            }
            .await;
            if let Err(error) = removed {
                failures.push(format!("{name}: {error}"));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; ").into())
        }
    }
}

pub struct McpReply {
    pub is_error: bool,
    pub value: Value,
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
    pub async fn start(directory: &Path, session: Option<&Path>) -> FixtureResult<Self> {
        Self::start_with_port_reports(directory, session, std::convert::identity)
            .await
            .map(|(host, _)| host)
    }

    /// Port reports can force rejection, but cannot permit a protected actual listener.
    pub async fn start_with_port_reports(
        directory: &Path,
        session: Option<&Path>,
        mut report_port: impl FnMut(u16) -> u16,
    ) -> FixtureResult<(Self, Vec<StoppedHost>)> {
        let mut rejected_ports = Vec::new();
        let mut stopped_attempts = Vec::new();
        for _ in 0..MAX_START_ATTEMPTS {
            let mut host = Self::spawn_owned(directory, session)?;
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

    fn spawn_owned(directory: &Path, session: Option<&Path>) -> FixtureResult<Self> {
        let binary = std::env::var_os("CODEX_ROUTER_E2E_HOST_BINARY")
            .map(PathBuf::from)
            .ok_or("set CODEX_ROUTER_E2E_HOST_BINARY to prebuilt target/debug/examples/board-debug-runtime")?;
        let stderr = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join(format!("host-{}.stderr", uuid::Uuid::now_v7())))?;
        let mut command = artifact_command(&binary)?;
        command.arg(directory).arg("0");
        let status_interval = directory.join("fixture-mail-status-interval-ms");
        if status_interval.is_file() {
            command.env(
                "CODEX_ROUTER_TEST_MAIL_STATUS_INTERVAL_MS",
                std::fs::read_to_string(status_interval)?.trim(),
            );
        }
        if let Some(session) = session {
            command.arg(session);
        }
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

    // Only takeover targets need the private replay observation.
    pub async fn local_cancel_delivery_recorded(
        &mut self,
        cancel: fleet_sync_protocol::ItemId,
    ) -> FixtureResult<bool> {
        let reply = self
            .query_project(json!({"kind":"localCancelDelivery", "cancel": cancel}))
            .await?;
        if reply.get("kind") != Some(&json!("localCancelDelivery"))
            || reply.get("cancel") != Some(&json!(cancel))
        {
            return Err("owned Host Local cancel observation identity mismatch".into());
        }
        reply
            .get("recorded")
            .and_then(Value::as_bool)
            .ok_or_else(|| "owned Host omitted Local cancel observation".into())
    }

    pub async fn replicate_project(&mut self, project: &str) -> FixtureResult {
        let reply = self
            .query_project(json!({"kind":"replicateProject", "project":project}))
            .await?;
        if reply != json!({"kind":"projectReplicated", "project":project}) {
            return Err("owned Host project replication identity mismatch".into());
        }
        Ok(())
    }

    pub async fn drain_project_publisher(&mut self, project: &str) -> FixtureResult {
        let reply = self
            .query_project(json!({"kind":"drainProjectPublisher","project":project}))
            .await?;
        if reply != json!({"kind":"projectPublisherIdle","project":project}) {
            return Err("owned Host publisher completion identity mismatch".into());
        }
        Ok(())
    }

    pub async fn seeded_handoff_frontier(&mut self, project: &str) -> FixtureResult<Value> {
        let reply = self
            .query_project(json!({"kind":"seededHandoffFrontier","project":project}))
            .await?;
        if reply.get("kind") != Some(&json!("seededHandoffFrontier"))
            || reply.get("project") != Some(&json!(project))
        {
            return Err("owned Host seed frontier observation identity mismatch".into());
        }
        reply
            .get("frontier")
            .cloned()
            .ok_or_else(|| "owned Host seed frontier is absent".into())
    }

    async fn query_project(&mut self, query: Value) -> FixtureResult<Value> {
        let mut request = serde_json::to_vec(&query)?;
        request.push(b'\n');
        self.child
            .stdin
            .as_mut()
            .ok_or("owned Host observation input is absent")?
            .write_all(&request)
            .await?;
        let mut line = String::new();
        if self.output.read_line(&mut line).await? == 0 {
            return Err("owned Host exited before Local cancel observation".into());
        }
        Ok(serde_json::from_str(&line)?)
    }

    pub async fn cli_output(&self, arguments: &[&OsStr]) -> FixtureResult<std::process::Output> {
        let mut command = artifact_command(Path::new(env!("CARGO_BIN_EXE_agent-collaboration")))?;
        command
            .args(arguments)
            .arg("--service-directory")
            .arg(&self.directory)
            .arg("--json");
        run_cli_output(command, CALL_BOUND).await
    }

    pub async fn cli(&self, arguments: &[&OsStr]) -> FixtureResult<Value> {
        let output = self.cli_output(arguments).await?;
        let reply: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
            format!(
                "CLI JSON failed: {error}; status={}; stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )
        })?;
        if !output.status.success() {
            return Err(format!(
                "CLI status {:?}: {reply}; stderr={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        reply
            .get("result")
            .cloned()
            .ok_or_else(|| "CLI omitted result envelope".into())
    }

    pub async fn mcp(&self, tool: &str, arguments: Value) -> FixtureResult<Value> {
        let reply = self.mcp_reply(tool, arguments).await?;
        if reply.is_error {
            return Err(format!("MCP tool refusal: {}", reply.value).into());
        }
        Ok(reply.value)
    }

    pub async fn mcp_reply(&self, tool: &str, arguments: Value) -> FixtureResult<McpReply> {
        let response = reqwest::Client::builder().no_proxy().timeout(CALL_BOUND).build()?
            .post(&self.mcp_url)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-11-25")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}}))
            .send().await?.error_for_status()?;
        let event_stream = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| h.contains("text/event-stream"));
        let text = String::from_utf8(response.bytes().await?.to_vec())?;
        let packets: Vec<Value> = if event_stream {
            text.replace("\r\n", "\n")
                .split("\n\n")
                .filter_map(|event| {
                    let data = event
                        .lines()
                        .filter_map(|line| line.strip_prefix("data:"))
                        .map(str::trim_start)
                        .collect::<Vec<_>>()
                        .join("\n");
                    (!data.is_empty()).then(|| serde_json::from_str(&data))
                })
                .collect::<Result<_, _>>()?
        } else {
            vec![serde_json::from_str(&text)?]
        };
        let reply = packets
            .into_iter()
            .find(|packet| packet.get("id") == Some(&json!(1)))
            .ok_or("MCP stream omitted the correlated reply")?;
        if let Some(error) = reply.get("error") {
            return Err(format!("MCP RPC refusal: {error}").into());
        }
        let result = reply.get("result").ok_or("MCP omitted result")?;
        let structured = result
            .get("structuredContent")
            .ok_or("MCP omitted structured content")?;
        for item in result
            .get("content")
            .and_then(Value::as_array)
            .ok_or("MCP omitted content")?
        {
            if item.get("type") == Some(&json!("text")) {
                let rendered: Value = serde_json::from_str(
                    item.get("text")
                        .and_then(Value::as_str)
                        .ok_or("MCP text content missing")?,
                )?;
                if &rendered != structured {
                    return Err("MCP rendered/structured results disagree".into());
                }
            }
        }
        Ok(McpReply {
            is_error: result.get("isError") == Some(&json!(true)),
            value: structured.clone(),
        })
    }

    pub async fn stop_for_readback(self) -> FixtureResult<StoppedHost> {
        let directory = self.directory.canonicalize()?;
        self.stop().await?;
        Ok(StoppedHost { directory })
    }

    /// Readback begins only after this owned child is killed, reaped and drained.
    // Some process-test roots only need graceful readback.
    pub async fn crash_for_readback(mut self) -> FixtureResult<StoppedHost> {
        use std::os::unix::process::ExitStatusExt;

        let directory = self.directory.canonicalize();
        let killed = self.child.start_kill();
        let mut remaining = Vec::new();
        let (waited, drained) = tokio::join!(
            tokio::time::timeout(STOP_BOUND, self.child.wait()),
            tokio::time::timeout(STOP_BOUND, self.output.read_to_end(&mut remaining)),
        );
        let mut failures = Vec::new();
        if let Err(error) = killed {
            failures.push(format!("owned child SIGKILL: {error}"));
        }
        match waited {
            Ok(Ok(status)) if status.signal() == Some(9) => {}
            Ok(Ok(status)) => failures.push(format!("expected owned child SIGKILL9, got {status}")),
            Ok(Err(error)) => failures.push(format!("owned child reap: {error}")),
            Err(error) => {
                failures.push(format!("owned child reap bound: {error}"));
                match tokio::time::timeout(STOP_BOUND, self.child.kill()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        failures.push(format!("owned child fallback kill/reap: {error}"))
                    }
                    Err(error) => {
                        failures.push(format!("owned child fallback kill/reap bound: {error}"))
                    }
                }
            }
        }
        match drained {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => failures.push(format!("owned child stdout drain: {error}")),
            Err(error) => failures.push(format!("owned child stdout drain bound: {error}")),
        }
        match directory {
            Ok(directory) if failures.is_empty() => {
                let stopped = StoppedHost { directory };
                stopped.remove_listener_residue().await?;
                Ok(stopped)
            }
            Ok(_) => Err(failures.join("; ").into()),
            Err(error) => {
                failures.push(format!("owned directory: {error}"));
                Err(failures.join("; ").into())
            }
        }
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
