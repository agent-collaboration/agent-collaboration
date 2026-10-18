//! V8: the real CLI and MCP tool share wake authority, bounds and typed refusals.
use automation_storage::AutomationStore;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_protocol::OperationId;
use collaboration_service::{CollaborationApplication, ServiceIdentity};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use tokio::sync::Mutex;

macro_rules! ensure {
    ($condition:expr $(,)?) => { ensure!($condition, "assertion failed: {}", stringify!($condition)); };
    ($condition:expr, $($context:tt)+) => {
        if !$condition { return Err(format!($($context)+).into()); }
    };
}
macro_rules! ensure_eq {
    ($left:expr, $right:expr $(,)?) => { ensure_eq!($left, $right, "values differ"); };
    ($left:expr, $right:expr, $($context:tt)+) => {{
        let left = &$left; let right = &$right;
        if left != right { return Err(format!("{}: left={left:?}, right={right:?}", format_args!($($context)+)).into()); }
    }};
}
macro_rules! ensure_ne {
    ($left:expr, $right:expr $(,)?) => {
        ensure!($left != $right, "values must differ");
    };
}
type TestResult<TValue = ()> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;
const SERVICE: &str = "00000000-0000-4000-8000-000000000001";

struct Surfaces {
    root: tempfile::TempDir,
    api: ServedCollaborationApi,
}
impl Surfaces {
    async fn start() -> TestResult<Self> {
        let root = tempfile::tempdir()?;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
        let store = Arc::new(Mutex::new(
            AutomationStore::open(&root.path().join("automation.sqlite")).await?,
        ));
        let identity = ServiceIdentity::new(SERVICE, SERVICE)?.with_automation_store(store);
        let api =
            ServedCollaborationApi::start(root.path(), CollaborationApplication::new(identity))
                .await?;
        Ok(Self { root, api })
    }
    fn session(id: &str) -> Value {
        json!({"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":id})
    }
    fn input(sender: &str) -> Value {
        json!({"operationId":OperationId::generate(),"requestedBy":{"kind":"session","endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":sender},"replace":false,
            "message":{"target":Self::session("wake-surface-target"),"content":{"kind":"agent","sender":Self::session(sender),"text":"Surface reminder"},"delivery":"auto"},
            "timing":{"kind":"after","seconds":600},"expiry":{"kind":"none"}})
    }
    async fn cli(&self, sender: &str, flags: &[&str]) -> TestResult<(i32, Value)> {
        let target = Self::session("wake-surface-target").to_string();
        let output = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
                .kill_on_drop(true)
                .env("CODEX_THREAD_ID", sender)
                .env_remove("CODEX_SESSION_ID")
                .env_remove("CLAUDE_CODE_SESSION_ID")
                .env_remove("CURSOR_CONVERSATION_ID")
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
                "CLI did not produce JSON: {error}; stderr={}",
                String::from_utf8_lossy(&output.stderr)
            )
        })?;
        Ok((output.status.code().unwrap_or(-1), wire))
    }
    async fn stop(self) -> TestResult {
        self.api.stop().await?;
        Ok(())
    }
}

fn same_refusal(cli: Value, mcp: Value, expected: Value) -> TestResult {
    let mut cli_error = cli
        .get("error")
        .and_then(Value::as_object)
        .ok_or("CLI error")?
        .clone();
    let mut mcp_error = mcp
        .pointer("/error/data")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("MCP refusal absent: {mcp}"))?
        .clone();
    cli_error.remove("operationId");
    mcp_error.remove("operationId");
    ensure_eq!(
        cli_error,
        mcp_error,
        "CLI and MCP changed refusal semantics"
    );
    for (field, expected) in expected.as_object().ok_or("expected refusal object")? {
        ensure_eq!(
            cli_error.get(field),
            Some(expected),
            "wrong refusal {field}"
        );
    }
    ensure_eq!(
        cli_error.get("effects"),
        Some(&json!({"kind":"local","mutation":"none"}))
    );
    Ok(())
}

#[tokio::test]
async fn cli_and_mcp_preserve_all_wake_limit_refusals_including_one_shot_expiry() -> TestResult {
    let surfaces = Surfaces::start().await?;
    let cases: Vec<(&[&str], Value, Value, Value)> = vec![
        (
            &["--after", "86401s"],
            json!({"kind":"after","seconds":86401}),
            json!({"kind":"none"}),
            json!({"kind":"tooFarAhead","max":86400}),
        ),
        (
            &["--after", "10m", "--for", "87001s"],
            json!({"kind":"after","seconds":600}),
            json!({"kind":"after","seconds":87001}),
            json!({"kind":"lifetimeTooLong","max":86400}),
        ),
        (
            &["--every", "5m"],
            json!({"kind":"interval","seconds":300}),
            json!({"kind":"none"}),
            json!({"kind":"expiryRequired"}),
        ),
        (
            &["--every", "5m", "--for", "604801s"],
            json!({"kind":"interval","seconds":300}),
            json!({"kind":"after","seconds":604801}),
            json!({"kind":"lifetimeTooLong","max":604800}),
        ),
        (
            &["--every", "299s", "--for", "2h"],
            json!({"kind":"interval","seconds":299}),
            json!({"kind":"after","seconds":7200}),
            json!({"kind":"intervalTooShort","min":300}),
        ),
        (
            &["--cron", "* * * * *", "--timezone", "UTC", "--for", "2h"],
            json!({"kind":"cron","expression":"* * * * *","timezone":"UTC"}),
            json!({"kind":"after","seconds":7200}),
            json!({"kind":"intervalTooShort","min":300}),
        ),
    ];
    for (flags, timing, expiry, expected) in cases {
        let (code, cli) = surfaces.cli("surface-creator", flags).await?;
        ensure_eq!(code, 4, "CLI must expose a definitive typed refusal: {cli}");
        let mut input = Surfaces::input("surface-creator");
        input["timing"] = timing;
        input["expiry"] = expiry;
        let mcp = surfaces.api.call("wake_send", input).await?;
        same_refusal(cli, mcp, expected)?;
    }
    let (code, valid) = surfaces
        .cli("surface-creator", &["--after", "10m", "--for", "24h"])
        .await?;
    ensure_eq!(code, 0, "exact one-shot expiry boundary refused: {valid}");
    let definition = valid
        .pointer("/result/record/definition")
        .ok_or("CLI wake definition")?;
    let start =
        chrono::DateTime::parse_from_rfc3339(definition["createdAt"].as_str().ok_or("createdAt")?)?;
    let end =
        chrono::DateTime::parse_from_rfc3339(definition["expiresAt"].as_str().ok_or("expiresAt")?)?;
    ensure_eq!(end - start, chrono::Duration::hours(24));
    // PD4 §7 (723ad984): the exact cap is 24h after due, not creation.
    let (code, exact) = surfaces
        .cli(
            "surface-creator",
            &["--after", "10m", "--for", "87000s", "--replace"],
        )
        .await?;
    ensure_eq!(code, 0, "exact due-relative end refused: {exact}");
    let mut input = Surfaces::input("surface-creator");
    input["replace"] = json!(true);
    input["expiry"] = json!({"kind":"after","seconds":87000});
    let mcp = surfaces.api.call("wake_send", input).await?;
    for definition in [
        exact
            .pointer("/result/record/definition")
            .ok_or("exact CLI definition")?,
        mcp.pointer("/result/definition")
            .ok_or("exact MCP definition")?,
    ] {
        let start = chrono::DateTime::parse_from_rfc3339(
            definition["createdAt"].as_str().ok_or("createdAt")?,
        )?;
        let end = chrono::DateTime::parse_from_rfc3339(
            definition["expiresAt"].as_str().ok_or("expiresAt")?,
        )?;
        ensure_eq!(end - start, chrono::Duration::seconds(87000));
    }
    surfaces.stop().await
}

#[tokio::test]
async fn cli_after_24_hours_keeps_its_due_time_and_due_relative_default_end() -> TestResult {
    let surfaces = Surfaces::start().await?;
    let (code, cli) = surfaces.cli("surface-creator", &["--after", "24h"]).await?;
    ensure_eq!(code, 0);
    let mut input = Surfaces::input("surface-creator");
    input["replace"] = json!(true);
    input["timing"] = json!({"kind":"after","seconds":86400});
    let mcp = surfaces.api.call("wake_send", input).await?;
    for record in [
        cli.pointer("/result/record").ok_or("CLI record")?,
        mcp.get("result").ok_or("MCP record")?,
    ] {
        let start = chrono::DateTime::parse_from_rfc3339(
            record["definition"]["createdAt"]
                .as_str()
                .ok_or("createdAt")?,
        )?;
        let due = chrono::DateTime::parse_from_rfc3339(
            record["nextDueAt"].as_str().ok_or("missing nextDueAt")?,
        )?;
        let end = chrono::DateTime::parse_from_rfc3339(
            record["definition"]["expiresAt"]
                .as_str()
                .ok_or("expiresAt")?,
        )?;
        ensure_eq!(due - start, chrono::Duration::hours(24));
        ensure_eq!(end - due, chrono::Duration::hours(24));
    }
    surfaces.stop().await
}

#[tokio::test]
async fn cli_and_mcp_keep_creator_and_target_replacement_and_slot_refusal_in_sync() -> TestResult {
    let surfaces = Surfaces::start().await?;
    let (code, first) = surfaces.cli("surface-creator", &["--after", "10m"]).await?;
    ensure_eq!(code, 0, "CLI creation failed: {first}");
    let id = first
        .pointer("/result/record/definition/wakeupId")
        .ok_or("first ID")?
        .clone();
    let creator = json!({"kind":"session","endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"surface-creator"});
    ensure_eq!(
        first.pointer("/result/record/definition/createdBy"),
        Some(&creator)
    );
    for replace in [false, true] {
        let flags: &[&str] = if replace {
            &["--after", "10m", "--replace"]
        } else {
            &["--after", "10m"]
        };
        let (code, cli) = surfaces.cli("unrelated-session", flags).await?;
        ensure_eq!(code, 4);
        let mut input = Surfaces::input("unrelated-session");
        input["replace"] = json!(replace);
        let mcp = surfaces.api.call("wake_send", input).await?;
        same_refusal(
            cli,
            mcp,
            json!({"kind":"wakeExists","wakeup":id,"creator":creator}),
        )?;
    }
    let (code, replacement) = surfaces
        .cli("surface-creator", &["--after", "10m", "--replace"])
        .await?;
    ensure_eq!(code, 0, "creator replacement failed: {replacement}");
    let next_id = replacement
        .pointer("/result/record/definition/wakeupId")
        .ok_or("replacement ID")?;
    ensure!(next_id != &id);
    let old = surfaces
        .api
        .call("wake_show", json!({"wakeupId":id}))
        .await?;
    ensure_eq!(old.pointer("/result/state"), Some(&json!("cancelled")));
    let mut target = Surfaces::input("wake-surface-target");
    target["replace"] = json!(true);
    let replaced = surfaces.api.call("wake_send", target).await?;
    ensure_eq!(
        replaced.pointer("/result/definition/createdBy/sessionId"),
        Some(&json!("wake-surface-target"))
    );
    ensure_ne!(
        replaced.pointer("/result/definition/wakeupId"),
        Some(next_id)
    );
    surfaces.stop().await
}
