//! Compiled-CLI proof, against a real Host, of what a Claude or Cursor create
//! must name: a mode (D1 spec R1), and a fast choice of off or auto only (R7).

#![expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]

use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use serde_json::Value;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

const CALLER_SESSION_ID: &str = "provider-create-mode-caller";
const HUMAN_CREATOR: &str = r#"{"kind":"human","humanId":"cli-owner"}"#;

/// A real Host whose Cursor endpoint runs `fixture` with `arguments`.
async fn start_host(root: &Path, fixture: &str, arguments: Vec<String>) -> CollaborationRuntime {
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))
        .expect("private Host root");
    Box::pin(CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            board_session: None,
            directory: root.to_owned(),
            codex_home: root.to_owned(),
            backend_socket: root.join("backend.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            native_schema: None,
            peer_registry_directory: None,
            remote_control_server_name: None,
            owner_human_id: None,
        },
        vec![ExternalProviderStartup::Launch(
            ExternalProviderLaunchBinding::cursor(
                node_test_fixture::node_executable().expect("Node"),
                std::iter::once(
                    node_test_fixture::fixture_program(fixture)
                        .to_string_lossy()
                        .into_owned(),
                )
                .chain(arguments)
                .collect(),
            )
            .expect("Cursor ACP binding"),
        )],
    ))
    .await
    .expect("real Host startup")
}

/// Runs the compiled CLI against the Host in `root`; returns its exit code and
/// its stdout and stderr together.
async fn run_cli(root: &Path, arguments: &[&str]) -> (Option<i32>, String) {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .arg("--service-directory")
            .arg(root)
            .env("CODEX_THREAD_ID", CALLER_SESSION_ID)
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CURSOR_CONVERSATION_ID")
            .output(),
    )
    .await
    .expect("CLI deadline")
    .expect("CLI output");
    (
        output.status.code(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    )
}

/// Creates a Cursor session as the human creator, with extra create flags.
async fn create(root: &Path, project: &Path, extra: &[&str]) -> (Option<i32>, String) {
    let project = project.display().to_string();
    let mut arguments = vec![
        "conversation",
        "create",
        "--endpoint",
        "cursor-local",
        "--cwd",
        project.as_str(),
        "--access",
        "workspace-write",
        "--from",
        HUMAN_CREATOR,
        "--json",
    ];
    arguments.extend_from_slice(extra);
    run_cli(root, &arguments).await
}

/// The created session's SessionRef JSON from the CLI's create output.
fn created_target(rendered: &str) -> String {
    rendered
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|line| line["kind"] == "created")
        .map(|line| line["target"].to_string())
        .expect("a created session")
}

/// Oracle: spec R1 "a Claude or Cursor create without a mode is refused": the real Host
/// answers InvalidSetting at stage Validation, and the CLI maps that kind to exit code 2
/// (`conversation_client_command_reporting.rs`, `report_create_operation_failure`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conversation_create_without_mode_is_refused() {
    // Arrange: a real Host with a Cursor provider, and a real working directory.
    let root = tempfile::tempdir().expect("Host root");
    let project = tempfile::tempdir().expect("project directory");
    let receipt = root.path().join("receipt.json");
    let runtime = start_host(
        root.path(),
        "permission-mode-agent.ts",
        vec![
            receipt.display().to_string(),
            "cursor".to_owned(),
            "no-directories".to_owned(),
        ],
    )
    .await;

    // Act: create a Cursor conversation without naming a mode.
    let (code, rendered) = create(root.path(), project.path(), &[]).await;

    // Assert: exit code 2, a message that names the way out, and no session
    // reached the agent.
    assert_eq!(code, Some(2), "{rendered}");
    assert!(rendered.contains("--mode auto"), "{rendered}");
    runtime.shutdown().await.expect("Host shutdown");
    let sessions_seen = std::fs::read(&receipt).map_or(serde_json::json!([]), |bytes| {
        serde_json::from_slice::<Value>(&bytes).expect("receipt JSON")["sessions"].clone()
    });
    assert_eq!(sessions_seen, serde_json::json!([]));
}

/// Oracle: D1 spec R1 and the Lead's decision on `conversation prompt --new`
/// (it names --mode like conversation create): without --mode the Host
/// refuses it with exit 2 naming --mode auto; with --mode auto it creates and
/// prompts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prompt_new_without_mode_is_refused_and_with_auto_works() {
    let root = tempfile::tempdir().expect("Host root");
    let project = tempfile::tempdir().expect("project directory");
    let receipt = root.path().join("wire.json");
    let runtime = start_host(
        root.path(),
        "cursor-settings-agent.ts",
        vec!["reactive".to_owned(), receipt.display().to_string()],
    )
    .await;
    let project = project.path().display().to_string();
    let prompt_new = |mode: Option<&'static str>| {
        let mut arguments = vec![
            "conversation",
            "prompt",
            "--new",
            "--endpoint",
            "cursor-local",
            "--cwd",
            project.as_str(),
            "--access",
            "workspace-write",
            "--text",
            "hello",
            "--json",
        ];
        if let Some(mode) = mode {
            arguments.extend_from_slice(&["--mode", mode]);
        }
        arguments
    };

    let (refused_code, refused) = run_cli(root.path(), &prompt_new(None)).await;
    let (auto_code, auto) = run_cli(root.path(), &prompt_new(Some("auto"))).await;
    runtime.shutdown().await.expect("Host shutdown");

    assert_eq!(refused_code, Some(2), "{refused}");
    assert!(refused.contains("--mode auto"), "{refused}");
    assert_eq!(auto_code, Some(0), "{auto}");
}

/// Oracle: D1 V13, spec R7 — `--fast` takes off or auto only (exit 2 for
/// anything else), and both start a session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_fast_flag_accepts_off_or_auto() {
    let root = tempfile::tempdir().expect("Host root");
    let project = tempfile::tempdir().expect("project directory");
    let receipt = root.path().join("wire.json");
    let runtime = start_host(
        root.path(),
        "cursor-settings-agent.ts",
        vec!["reactive".to_owned(), receipt.display().to_string()],
    )
    .await;
    let mut outcomes = Vec::new();
    for fast in ["on", "true", "off", "auto"] {
        let (code, rendered) = create(
            root.path(),
            project.path(),
            &["--mode", "auto", "--fast", fast],
        )
        .await;
        outcomes.push((fast, code, rendered));
    }
    runtime.shutdown().await.expect("Host shutdown");
    for (fast, code, rendered) in outcomes {
        let expected = if matches!(fast, "off" | "auto") { 0 } else { 2 };
        assert_eq!(code, Some(expected), "--fast {fast}: {rendered}");
    }
}

/// Oracle: D1 V13, spec R7 — `settings set --setting fast` takes off or auto;
/// the Host refuses anything else with exit 2.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settings_set_fast_accepts_off_or_auto() {
    let root = tempfile::tempdir().expect("Host root");
    let project = tempfile::tempdir().expect("project directory");
    let receipt = root.path().join("wire.json");
    let runtime = start_host(
        root.path(),
        "cursor-settings-agent.ts",
        vec!["reactive".to_owned(), receipt.display().to_string()],
    )
    .await;
    let (code, rendered) = create(root.path(), project.path(), &["--mode", "auto"]).await;
    assert_eq!(code, Some(0), "{rendered}");
    let target = created_target(&rendered);
    let mut outcomes = Vec::new();
    for value in ["on", "true", "off", "auto"] {
        let (code, rendered) = run_cli(
            root.path(),
            &[
                "conversation",
                "settings",
                "set",
                "--target",
                target.as_str(),
                "--actor",
                HUMAN_CREATOR,
                "--setting",
                "fast",
                "--value",
                value,
                "--json",
            ],
        )
        .await;
        outcomes.push((value, code, rendered));
    }
    runtime.shutdown().await.expect("Host shutdown");
    for (value, code, rendered) in outcomes {
        let expected = if matches!(value, "off" | "auto") {
            0
        } else {
            2
        };
        assert_eq!(code, Some(expected), "--value {value}: {rendered}");
    }
}
