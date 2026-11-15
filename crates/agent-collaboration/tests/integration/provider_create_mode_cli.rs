//! Compiled-CLI proof that a Claude or Cursor create without a mode is refused by the real Host.

use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use std::os::unix::fs::PermissionsExt as _;

const CALLER_SESSION_ID: &str = "provider-create-mode-caller";

/// Oracle: spec R1 "a Claude or Cursor create without a mode is refused": the real Host
/// answers InvalidSetting at stage Validation, and the CLI maps that kind to exit code 2
/// (`conversation_client_command_reporting.rs`, `report_create_operation_failure`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
async fn conversation_create_without_mode_is_refused() {
    // Arrange: a real Host with a Cursor provider, and a real working directory.
    let root = tempfile::tempdir().expect("Host root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private Host root");
    let project = tempfile::tempdir().expect("project directory");
    let receipt = root.path().join("receipt.json");
    let runtime = Box::pin(CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            board_session: None,
            directory: root.path().to_owned(),
            codex_home: root.path().to_owned(),
            backend_socket: root.path().join("backend.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            native_schema: None,
            peer_registry_directory: None,
            remote_control_server_name: None,
            owner_human_id: None,
        },
        vec![ExternalProviderStartup::Launch(
            ExternalProviderLaunchBinding::cursor(
                node_test_fixture::node_executable().expect("Node"),
                vec![
                    node_test_fixture::fixture_program("permission-mode-agent.ts")
                        .to_string_lossy()
                        .into_owned(),
                    receipt.display().to_string(),
                    "cursor".to_owned(),
                    "no-directories".to_owned(),
                ],
            )
            .expect("Cursor ACP binding"),
        )],
    ))
    .await
    .expect("real Host startup");

    // Act: create a Cursor conversation without naming a mode.
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args([
                "conversation",
                "create",
                "--endpoint",
                "cursor-local",
                "--cwd",
            ])
            .arg(project.path())
            .args([
                "--access",
                "workspace-write",
                "--json",
                "--service-directory",
            ])
            .arg(root.path())
            .env("CODEX_THREAD_ID", CALLER_SESSION_ID)
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CURSOR_CONVERSATION_ID")
            .output(),
    )
    .await
    .expect("CLI deadline")
    .expect("CLI output");

    // Assert: exit code 2, a message that names the way out, and no session
    // reached the agent.
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(2), "{rendered}");
    assert!(rendered.contains("--mode auto"), "{rendered}");
    runtime.shutdown().await.expect("Host shutdown");
    let sessions_seen = std::fs::read(&receipt).map_or(serde_json::json!([]), |bytes| {
        serde_json::from_slice::<serde_json::Value>(&bytes).expect("receipt JSON")["sessions"]
            .clone()
    });
    assert_eq!(sessions_seen, serde_json::json!([]));
}
