//! Two real enrolled Hosts share central; every owned task retires before fixture removal.
use board_central_test_support::{
    EnrolledTestMachine, ObjectAddress, RuntimeControl, RuntimeControlRequest, TestCentral,
};
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_central_conformance::AutomationClockControl;
use serde_json::json;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};

pub type ProofResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;
pub struct CentralWakePair {
    pub first: CollaborationRuntime,
    pub second: CollaborationRuntime,
    pub first_root: PathBuf,
    pub second_root: PathBuf,
    pub due: String,
    machine: EnrolledTestMachine,
    central: TestCentral,
    directory: tempfile::TempDir,
}
impl CentralWakePair {
    pub async fn start() -> ProofResult<Self> {
        let signer = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/cargo_debug_signing_runner.sh");
        let signed = tokio::process::Command::new(signer)
            .args(["--sign-only", env!("CARGO_BIN_EXE_agent-collaboration")])
            .kill_on_drop(true)
            .status()
            .await?;
        if !signed.success() {
            return Err("CLI signing failed".into());
        }
        let directory = tempfile::Builder::new()
            .prefix("wake-pair-")
            .tempdir_in("/tmp")?;
        let first_root = directory.path().join("a");
        let second_root = directory.path().join("b");
        let first_origin = initialize(&first_root)?;
        let second_origin = initialize(&second_root)?;
        let due_ms = collaboration_protocol::GridTime::next_after(
            chrono::Utc::now().timestamp_millis() - 1_800_000,
        )?
        .unix_millis();
        let timestamp = |millis| {
            chrono::DateTime::from_timestamp_millis(millis)
                .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                .ok_or("fixture timestamp")
        };
        let due = timestamp(due_ms)?;
        let central = TestCentral::start_with_peer_and_controls(
            &directory.path().join("central"),
            &first_origin,
            collaboration_runtime::load_installation_peer(&first_root)?,
            vec![RuntimeControlRequest {
                address: ObjectAddress::Fleet,
                command: RuntimeControl::AutomationClock(AutomationClockControl::Start {
                    now: serde_json::from_value(json!(timestamp(due_ms - 1)?))?,
                }),
            }],
        )
        .await?;
        let machine = match central
            .additional_machine(
                &second_origin,
                collaboration_runtime::load_installation_peer(&second_root)?,
            )
            .await
        {
            Ok(machine) => machine,
            Err(error) => {
                central.shutdown().await?;
                return Err(error);
            }
        };
        let first = match start(&first_root, central.session_inputs()?).await {
            Ok(runtime) => runtime,
            Err(error) => {
                machine.shutdown().await?;
                central.shutdown().await?;
                return Err(error.into());
            }
        };
        let second = match start(&second_root, machine.session_inputs()?).await {
            Ok(runtime) => runtime,
            Err(error) => {
                first.shutdown().await?;
                machine.shutdown().await?;
                central.shutdown().await?;
                return Err(error.into());
            }
        };
        let fixture = Self {
            first,
            second,
            first_root,
            second_root,
            due,
            machine,
            central,
            directory,
        };
        let ready: ProofResult = async {
            tokio::time::timeout(
                Duration::from_secs(5),
                fixture
                    .first
                    .runtime_test_causal_machine_work_readiness_pull(),
            )
            .await??;
            tokio::time::timeout(
                Duration::from_secs(5),
                fixture
                    .second
                    .runtime_test_causal_machine_work_readiness_pull(),
            )
            .await??;
            Ok(())
        }
        .await;
        if let Err(error) = ready {
            fixture.close().await?;
            return Err(error);
        }
        Ok(fixture)
    }
    pub async fn run_cli(&self, arguments: &[&str], sender: &str) -> ProofResult<Output> {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"));
        command
            .env_clear()
            .env("PATH", "")
            .env("HOME", self.first_root.join("cli-home"))
            .env("CODEX_HOME", self.first_root.join("codex-home"))
            .env("CODEX_THREAD_ID", sender)
            .args(arguments)
            .arg("--service-directory")
            .arg(&self.first_root)
            .arg("--json")
            .kill_on_drop(true);
        Ok(tokio::time::timeout(Duration::from_secs(10), command.output()).await??)
    }
    pub async fn close(self) -> ProofResult {
        let second = self.second.shutdown().await;
        let first = self.first.shutdown().await;
        let machine = self.machine.shutdown().await;
        let central = self.central.shutdown().await;
        let directory = self.directory.close();
        second?;
        first?;
        machine?;
        central?;
        directory?;
        Ok(())
    }
}
fn initialize(root: &Path) -> ProofResult<message_board::ActorOrigin> {
    std::fs::create_dir(root)?;
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    for child in ["codex-home", "cli-home", "peers"] {
        std::fs::create_dir(root.join(child))?;
        std::fs::set_permissions(root.join(child), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(message_board::ActorOrigin {
        installation: String::from(collaboration_runtime::load_service_identity(root)?)
            .try_into()?,
        machine: "Wake CLI two-Host fixture".to_owned().try_into()?,
    })
}
async fn start(
    root: &Path,
    enrollment: board_fleet_sync::BoardSessionInputs,
) -> std::io::Result<CollaborationRuntime> {
    CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: Some(enrollment),
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
}
