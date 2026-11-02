//! One real enrolled Host, central and CLI process; no native backend or model execution.
use board_central_test_support::{
    ObjectAddress, RuntimeControl, RuntimeControlRequest, TestCentral,
};
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_central_conformance::{AutomationClockControl, ConformanceClient};
use collaboration_protocol::{GridTime, ObservationTimestamp};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path, process::Output, time::Duration};

pub type ProofResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;

pub fn directory() -> ProofResult<tempfile::TempDir> {
    let root = tempfile::Builder::new()
        .prefix("central-cli-")
        .tempdir_in("/tmp")?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    Ok(root)
}

pub struct CentralHost {
    pub runtime: CollaborationRuntime,
    pub due: ObservationTimestamp,
    central: TestCentral,
    directory: tempfile::TempDir,
}
impl CentralHost {
    pub async fn start(directory: tempfile::TempDir) -> ProofResult<Self> {
        let root = directory.path();
        // Cargo's runner signs the test itself; this executable is launched directly below.
        let signer = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/cargo_debug_signing_runner.sh");
        let status = tokio::process::Command::new(signer)
            .arg("--sign-only")
            .arg(env!("CARGO_BIN_EXE_agent-collaboration"))
            .kill_on_drop(true)
            .status()
            .await?;
        if !status.success() {
            return Err("CLI debug signing setup failed".into());
        }
        for child in ["codex-home", "cli-home", "peers"] {
            std::fs::create_dir(root.join(child))?;
            std::fs::set_permissions(root.join(child), std::fs::Permissions::from_mode(0o700))?;
        }
        let identity = collaboration_contracts::MachineIdentity::new(
            collaboration_runtime::load_service_identity(root)?,
            None,
        )?;
        // Only the central clock triggers these occurrences; the Host independently admits
        // due work against its existing UTC clock. The finite expiry remains in the future.
        let due_ms =
            GridTime::next_after(chrono::Utc::now().timestamp_millis() - 1_800_000)?.unix_millis();
        let due = collaboration_runtime::observation_timestamp::timestamp(due_ms)
            .map_err(|()| "CLI fixture due time")?;
        let initial = collaboration_runtime::observation_timestamp::timestamp(due_ms - 1)
            .map_err(|()| "CLI fixture clock")?;
        let central = TestCentral::start_with_peer_and_controls(
            &root.join("central"),
            &message_board::ActorOrigin {
                installation: String::from(identity.service_id().clone()).try_into()?,
                machine: identity.machine_label().as_str().to_owned().try_into()?,
            },
            collaboration_runtime::load_installation_peer(root)?,
            vec![RuntimeControlRequest {
                address: ObjectAddress::Fleet,
                command: RuntimeControl::AutomationClock(AutomationClockControl::Start {
                    now: serde_json::from_value(json!(initial))?,
                }),
            }],
        )
        .await?;
        let runtime = match CollaborationRuntime::start(CollaborationRuntimeInputs {
            board_session: Some(central.session_inputs()?),
            directory: root.to_owned(),
            codex_home: root.join("codex-home"),
            backend_socket: root.join("absent-native.sock"),
            mcp_bind: ([127, 0, 0, 1], 0).into(),
            native_schema: None,
            peer_registry_directory: Some(root.join("peers")),
            remote_control_server_name: None,
            owner_human_id: None,
        })
        .await
        {
            Ok(runtime) => runtime,
            Err(error) => {
                central.shutdown().await?;
                return Err(error.into());
            }
        };
        let ready = tokio::time::timeout(
            Duration::from_secs(5),
            runtime.runtime_test_causal_machine_work_readiness_pull(),
        )
        .await;
        if !matches!(ready, Ok(Ok(()))) {
            let stopped = runtime.shutdown().await;
            let central_stopped = central.shutdown().await;
            stopped?;
            central_stopped?;
            return Err(format!("CLI machine-work readiness failed: {ready:?}").into());
        }
        Ok(Self {
            runtime,
            due,
            central,
            directory,
        })
    }
    pub fn root(&self) -> &Path {
        self.directory.path()
    }
    pub async fn run_cli(&self, arguments: &[&str]) -> ProofResult<Output> {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"));
        command
            .env_clear()
            .env("PATH", "")
            .env("HOME", self.root().join("cli-home"))
            .env("CODEX_HOME", self.root().join("codex-home"))
            .env("CODEX_THREAD_ID", "wake-cli-sender")
            .args(arguments)
            .arg("--service-directory")
            .arg(self.root())
            .kill_on_drop(true);
        Ok(tokio::time::timeout(Duration::from_secs(10), command.output()).await??)
    }
    pub async fn json(&self, arguments: &[&str]) -> ProofResult<Value> {
        let mut arguments = arguments.to_vec();
        arguments.push("--json");
        let output = self.run_cli(&arguments).await?;
        if !output.status.success() {
            return Err(format!(
                "CLI {arguments:?} failed: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(serde_json::from_slice(&output.stdout)?)
    }
    pub async fn advance_to_due(&self) -> ProofResult {
        ConformanceClient::new(self.central.base_url())?
            .automation_clock(AutomationClockControl::Advance {
                now: serde_json::from_value(json!(self.due))?,
            })
            .await?;
        Ok(())
    }
    pub async fn close(self) -> ProofResult {
        let stopped = self.runtime.shutdown().await;
        let central = self.central.shutdown().await;
        let removed = self.directory.close();
        stopped?;
        central?;
        removed?;
        Ok(())
    }
}
