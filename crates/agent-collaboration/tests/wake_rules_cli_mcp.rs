//! CLI and MCP preserve central grid bounds, creator slots and replacement authority.
use automation_storage::AutomationStore;
use board_central_test_support::{
    ObjectAddress, RuntimeControl, RuntimeControlRequest, TestCentral,
};
use collaboration_central_conformance::AutomationClockControl;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, MachineWorkProtocol, OperationId, PullAutomationBatches,
};
use collaboration_runtime::{CentralAutomationTransport, SessionAutomationTransport};
use collaboration_service::ApplicationFixture;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use tokio::sync::Mutex;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const NOW: &str = "2026-11-01T12:00:01Z";
const DUE: &str = "2026-11-01T12:05:00Z";

struct Surfaces {
    root: tempfile::TempDir,
    api: ServedCollaborationApi,
    central: board_central_test_support::FleetTestRuntime,
    transport: Arc<dyn CentralAutomationTransport>,
    store: Arc<Mutex<AutomationStore>>,
}
impl Surfaces {
    async fn start() -> TestResult<Self> {
        let root = tempfile::Builder::new()
            .prefix("wake-surfaces-")
            .tempdir_in("/tmp")?;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
        for child in ["cli-home", "codex-home"] {
            std::fs::create_dir(root.path().join(child))?;
            std::fs::set_permissions(
                root.path().join(child),
                std::fs::Permissions::from_mode(0o700),
            )?;
        }
        let signer = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/cargo_debug_signing_runner.sh");
        if !tokio::process::Command::new(signer)
            .args(["--sign-only", env!("CARGO_BIN_EXE_agent-collaboration")])
            .kill_on_drop(true)
            .status()
            .await?
            .success()
        {
            return Err("CLI debug signing setup failed".into());
        }
        let origin = message_board::ActorOrigin {
            installation: SERVICE.to_owned().try_into()?,
            machine: "CLI and MCP bounds fixture".to_owned().try_into()?,
        };
        let central = TestCentral::start_with_peer_and_controls(
            &root.path().join("central"),
            &origin,
            "019f0000-0000-7000-8000-000000001f15"
                .to_owned()
                .try_into()?,
            vec![RuntimeControlRequest {
                address: ObjectAddress::Fleet,
                command: RuntimeControl::AutomationClock(AutomationClockControl::Start {
                    now: serde_json::from_value(json!(NOW))?,
                }),
            }],
        )
        .await?;
        let (requests, central) = central.connect_machine_session(root.path()).await?;
        let transport: Arc<dyn CentralAutomationTransport> =
            Arc::new(SessionAutomationTransport::new(requests));
        if !matches!(
            transport
                .call(AutomationCall::PullAutomationBatches(
                    PullAutomationBatches {
                        after: None,
                        max_bytes: 1_048_576,
                        work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
                    }
                ))
                .await?,
            AutomationAnswer::Batches(_)
        ) {
            return Err("fixture did not advertise actual MachineWorkV1".into());
        }
        let store = Arc::new(Mutex::new(
            AutomationStore::open(&root.path().join("automation.sqlite")).await?,
        ));
        let application = ApplicationFixture::new(SERVICE, SERVICE)?
            .with_automation_store(store.clone())
            .with_catalog(Some(transport.clone()))
            .build();
        let api = ServedCollaborationApi::start(root.path(), application)?;
        Ok(Self {
            root,
            api,
            central,
            transport,
            store,
        })
    }
    fn session(id: &str) -> Value {
        json!({"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":id})
    }
    fn input(sender: &str) -> Value {
        json!({"operationId":OperationId::generate(),"requestedBy":{"kind":"session","endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":sender},"replace":false,
            "message":{"target":Self::session("wake-surface-target"),"content":{"kind":"agent","sender":Self::session(sender),"text":"Surface reminder"},"delivery":"auto"},
            "timing":{"kind":"at","at":DUE},"expiry":{"kind":"none"}})
    }
    async fn cli(&self, sender: &str, flags: &[&str]) -> TestResult<(i32, Value)> {
        let target = Self::session("wake-surface-target").to_string();
        let output = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
                .kill_on_drop(true)
                .env_clear()
                .env("PATH", "")
                .env("HOME", self.root.path().join("cli-home"))
                .env("CODEX_HOME", self.root.path().join("codex-home"))
                .env("CODEX_THREAD_ID", sender)
                .args([
                    "wake",
                    "send",
                    "--to",
                    &target,
                    "--text",
                    "Surface reminder",
                    "--json",
                    "--service-directory",
                ])
                .arg(self.root.path())
                .args(flags)
                .output(),
        )
        .await??;
        let wire = serde_json::from_slice(&output.stdout).map_err(|error| {
            format!(
                "CLI omitted JSON: {error}; stderr={}",
                String::from_utf8_lossy(&output.stderr),
            )
        })?;
        Ok((output.status.code().unwrap_or(-1), wire))
    }
    async fn stop(self) -> TestResult {
        let Self {
            root,
            api,
            central,
            transport,
            store,
        } = self;
        let stopped = api.stop().await;
        let central = central.shutdown().await;
        drop(transport);
        let storage = Arc::try_unwrap(store)
            .map_err(|_| "surface store still owned")?
            .into_inner()
            .close()
            .await;
        stopped?;
        central?;
        storage?;
        root.close()?;
        Ok(())
    }
}
fn same_refusal(cli: Value, mcp: Value, expected: Value) -> TestResult {
    let mut cli_error = cli
        .get("error")
        .and_then(Value::as_object)
        .ok_or("CLI error missing")?
        .clone();
    let mut mcp_error = mcp
        .pointer("/error/data")
        .and_then(Value::as_object)
        .ok_or("MCP typed refusal missing")?
        .clone();
    if !cli_error
        .remove("operationId")
        .is_some_and(|id| id.is_string())
        || !mcp_error
            .remove("operationId")
            .is_some_and(|id| id.is_string())
        || cli_error != mcp_error
        || cli_error.get("kind") != Some(&json!("refused"))
        || cli_error.get("error") != Some(&expected)
        || cli_error.contains_key("effects")
    {
        return Err(format!("CLI/MCP changed central refusal: CLI={cli_error:?}; MCP={mcp_error:?}; expected={expected}").into());
    }
    Ok(())
}
fn limit(name: &str, value: u64, minimum: u64, maximum: u64, step: u64) -> Value {
    json!({"kind":"validation","error":{"kind":"limitExceeded","limit":name,"value":value,"minimum":minimum,"maximum":maximum,"step":step}})
}
#[tokio::test]
async fn cli_and_mcp_preserve_central_limits_and_exact_due_relative_end() -> TestResult {
    let surfaces = Surfaces::start().await?;
    let proof: TestResult = async {
        let cases: Vec<(&[&str], Value, Value, Value)> = vec![
            (
                &["--at", "2026-11-02T12:05:00Z"],
                json!({"kind":"at","at":"2026-11-02T12:05:00Z"}),
                json!({"kind":"none"}),
                limit("WAKE_ONE_SHOT_MAX_AHEAD", 86699, 0, 86400, 1),
            ),
            (
                &["--at", DUE, "--for", "86700s"],
                json!({"kind":"at","at":DUE}),
                json!({"kind":"after","seconds":86700}),
                limit("WAKE_ONE_SHOT_MAX_AHEAD", 86401, 0, 86400, 1),
            ),
            (
                &["--every", "10m"],
                json!({"kind":"interval","seconds":600}),
                json!({"kind":"none"}),
                json!({"kind":"wakeExpiryRequired"}),
            ),
            (
                &["--every", "10m", "--for", "604801s"],
                json!({"kind":"interval","seconds":600}),
                json!({"kind":"after","seconds":604801}),
                limit("WAKE_REPEAT_MAX_LIFETIME", 604801, 1, 604800, 1),
            ),
            (
                &["--every", "5m", "--for", "2h"],
                json!({"kind":"interval","seconds":300}),
                json!({"kind":"after","seconds":7200}),
                limit("WAKE_MIN_PERIOD", 300, 600, 31536000, 300),
            ),
            (
                &["--cron", "*/5 * * * *", "--timezone", "UTC", "--for", "2h"],
                json!({"kind":"cron","expression":"*/5 * * * *","timezone":"UTC"}),
                json!({"kind":"after","seconds":7200}),
                limit("WAKE_MIN_PERIOD", 300, 600, 31536000, 300),
            ),
        ];
        for (flags, timing, expiry, expected) in cases {
            let (code, cli) = surfaces.cli("surface-creator", flags).await?;
            if code != 4 {
                return Err(format!("CLI omitted definitive central refusal: {cli}").into());
            }
            let mut input = Surfaces::input("surface-creator");
            *input.get_mut("timing").ok_or("timing missing")? = timing;
            *input.get_mut("expiry").ok_or("expiry missing")? = expiry;
            same_refusal(cli, surfaces.api.call("wake_send", input).await?, expected)?;
        }
        let (code, valid) = surfaces
            .cli("surface-creator", &["--at", DUE, "--for", "24h"])
            .await?;
        if code != 0
            || valid.pointer("/result/record/value/definition/expiresAt")
                != Some(&json!("2026-11-02T12:00:01Z"))
        {
            return Err(format!("finite one-shot expiry changed: {valid}").into());
        }
        let (code, exact) = surfaces
            .cli(
                "surface-creator",
                &["--at", DUE, "--for", "86699s", "--replace"],
            )
            .await?;
        let mut input = Surfaces::input("surface-creator");
        *input.get_mut("replace").ok_or("replace missing")? = json!(true);
        *input.get_mut("expiry").ok_or("expiry missing")? = json!({"kind":"after","seconds":86699});
        let mcp = surfaces.api.call("wake_send", input).await?;
        if code != 0
            || exact.pointer("/result/record/value/definition/expiresAt")
                != Some(&json!("2026-11-02T12:05:00Z"))
            || mcp.pointer("/result/value/definition/expiresAt")
                != Some(&json!("2026-11-02T12:05:00Z"))
        {
            return Err(format!("CLI/MCP changed exact due-relative cap: {exact}; {mcp}").into());
        }
        Ok(())
    }
    .await;
    let stopped = surfaces.stop().await;
    proof?;
    stopped
}
#[tokio::test]
async fn cli_and_mcp_refuse_after_and_off_grid_period_with_exact_neighbours() -> TestResult {
    let surfaces = Surfaces::start().await?;
    let proof: TestResult = async {
        for (flags,timing,field,earlier,later) in [
            (["--after","24h","--for","2h"],json!({"kind":"after","seconds":86400}),"after is not supported for automations","2026-11-02T12:00:00Z","2026-11-02T12:05:00Z"),
            (["--every","299s","--for","2h"],json!({"kind":"interval","seconds":299}),"seconds","2026-11-01T12:05:00Z","2026-11-01T12:10:00Z"),
        ] {
            let (code,cli) = surfaces.cli("surface-creator", &flags).await?;
            if code != 4 { return Err("CLI accepted retired After or an off-grid period".into()); }
            let mut input = Surfaces::input("surface-creator");
            *input.get_mut("timing").ok_or("timing missing")? = timing;
            *input.get_mut("expiry").ok_or("expiry missing")? = json!({"kind":"after","seconds":7200});
            same_refusal(cli,surfaces.api.call("wake_send",input).await?,json!({"kind":"validation","error":{"kind":"offGrid","field":field,"earlier":earlier,"later":later}}))?;
        }
        let page = surfaces.api.call("wake_list",json!({"limit":100})).await?;
        if page.pointer("/result/records").and_then(Value::as_array).is_none_or(|records| !records.is_empty()) {
            return Err("refused wake created catalog state".into());
        }
        Ok(())
    }.await;
    let stopped = surfaces.stop().await;
    proof?;
    stopped
}
#[tokio::test]
async fn cli_and_mcp_keep_creator_and_target_replacement_and_slot_refusal_in_sync() -> TestResult {
    let surfaces = Surfaces::start().await?;
    let proof: TestResult = async {
        let (code,first) = surfaces.cli("surface-creator", &["--at",DUE]).await?;
        if code != 0 { return Err(format!("CLI creation failed: {first}").into()); }
        let id = first.pointer("/result/record/value/definition/wakeupId").ok_or("first ID missing")?.clone();
        let creator = json!({"kind":"session","endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"surface-creator"});
        if first.pointer("/result/record/value/definition/createdBy") != Some(&creator) { return Err("CLI lost creator identity".into()); }
        for replace in [false,true] {
            let flags: &[&str] = if replace { &["--at",DUE,"--replace"] } else { &["--at",DUE] };
            let (code,cli) = surfaces.cli("unrelated-session",flags).await?;
            if code != 4 { return Err("unrelated creator claimed the central slot".into()); }
            let mut input = Surfaces::input("unrelated-session");
            *input.get_mut("replace").ok_or("replace missing")? = json!(replace);
            same_refusal(cli,surfaces.api.call("wake_send",input).await?,json!({"kind":"wakeExists","creator":creator}))?;
        }
        let (code,replacement) = surfaces.cli("surface-creator", &["--at",DUE,"--replace"]).await?;
        let next_id = replacement.pointer("/result/record/value/definition/wakeupId").ok_or("replacement ID missing")?;
        if code != 0 || next_id == &id { return Err("creator replacement did not allocate a new Wake".into()); }
        let old = surfaces.api.call("wake_show",json!({"wakeupId":id})).await?;
        if old.pointer("/result/value/state") != Some(&json!("cancelled")) { return Err("replacement failed to cancel old Wake".into()); }
        let mut target = Surfaces::input("wake-surface-target");
        *target.get_mut("replace").ok_or("replace missing")? = json!(true);
        let replaced = surfaces.api.call("wake_send",target).await?;
        if replaced.pointer("/result/value/definition/createdBy/sessionId") != Some(&json!("wake-surface-target"))
            || replaced.pointer("/result/value/definition/wakeupId").is_none_or(|id| id == next_id) {
            return Err("target replacement lost authority or reused the old Wake identity".into());
        }
        Ok(())
    }.await;
    let stopped = surfaces.stop().await;
    proof?;
    stopped
}
