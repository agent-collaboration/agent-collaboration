//! The real Host command port behind the Router's ACP socket, which carries no
//! mode in `session/new` (D1 spec R1: such doors ask for auto by name).
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use collaboration_protocol::{ProviderSessionInspectRequest, SessionId, SessionRef};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt as _;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

/// Writes an executable that runs the permission-mode fixture as Claude.
#[expect(
    clippy::expect_used,
    reason = "A fixture that cannot be written must fail its test."
)]
/// The Claude permission-mode fixture launched by Node itself, so the Host
/// knows the Node that runs Claude's work-roots hook (D1 R18).
fn claude_permission_fixture(root: &std::path::Path) -> (std::path::PathBuf, Vec<String>) {
    let program = node_test_fixture::fixture_program("permission-mode-agent.ts")
        .canonicalize()
        .expect("fixture program");
    let node = node_test_fixture::node_executable().expect("node");
    (
        node,
        vec![
            program.display().to_string(),
            root.join("receipt.json").display().to_string(),
            "claude".to_owned(),
            "grants-directories".to_owned(),
        ],
    )
}

async fn next_json(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
) -> Value {
    // The test runner bounds the case (`.config/nextest.toml`).
    let line = lines.next_line().await.ok().flatten().unwrap_or_default();
    serde_json::from_str(&line).unwrap_or(Value::Null)
}

/// Oracle: D1 spec R1 and R2 row "Confirms auto" — an ACP `session/new` names
/// no mode, so the door asks for auto; the real Host create through
/// HostSessionCommandPort starts it with requested auto and actual auto.
#[tokio::test]
async fn protocol_door_create_succeeds_in_auto() {
    let root = tempfile::tempdir().expect("runtime root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private runtime root");
    let (provider, provider_arguments) = claude_permission_fixture(root.path());
    let runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            board_session: None,
            owner_human_id: Some(
                message_board::HumanId::try_from("acp-owner".to_owned()).expect("owner"),
            ),
            directory: root.path().to_owned(),
            codex_home: root.path().to_owned(),
            backend_socket: root.path().join("backend.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            native_schema: None,
            peer_registry_directory: None,
            remote_control_server_name: None,
        },
        vec![ExternalProviderStartup::Launch({
            let mut binding = ExternalProviderLaunchBinding::claude(provider, provider_arguments)
                .expect("provider binding");
            codex_router_test_support::global_claude_fixture::declare_global_claude_fixture(
                root.path(),
                &mut binding.launch.environment,
            )
            .expect("declare test-owned global Claude executable");
            binding
        })],
    )
    .await
    .expect("Host starts provider face");
    let client = collaboration_client::CollaborationClient::connect(root.path(), "door-proof", "1")
        .await
        .expect("collaboration API client");
    let claude = client
        .list_endpoints()
        .await
        .expect("endpoint inventory")
        .endpoints
        .into_iter()
        .find(|endpoint| String::from(endpoint.endpoint.endpoint_id.clone()) == "claude-local")
        .expect("Claude endpoint")
        .endpoint;

    let stream = UnixStream::connect(root.path().join("codex-acp.sock"))
        .await
        .expect("ACP socket");
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    for request in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":1,"clientCapabilities":{},"_meta":{
                "router":{"actor":{"kind":"human","humanId":"acp-owner"}},
                "sessionProfile":{"version":1,"elements":[]}
            }
        }}),
        json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{
            "cwd":root.path(),"mcpServers":[],"_meta":{"router":{"endpoint":"claude-local"}}
        }}),
    ] {
        write
            .write_all(format!("{request}\n").as_bytes())
            .await
            .expect("ACP request");
        let answer = next_json(&mut lines).await;
        assert_eq!(answer["id"], request["id"], "{answer}");
        assert!(answer.get("error").is_none(), "{answer}");
    }
    let inspected = client
        .inspect_provider_session(ProviderSessionInspectRequest {
            target: SessionRef {
                endpoint: claude,
                session_id: SessionId::try_from("fixture-session-1".to_owned())
                    .expect("session id"),
            },
        })
        .await
        .expect("show the started session");
    runtime.shutdown().await.expect("Host shutdown");

    assert_eq!(inspected.requested_mode.as_deref(), Some("auto"));
    assert_eq!(inspected.mode_reason, None);
    assert_eq!(
        inspected
            .settings_catalog
            .expect("settings catalog")
            .current_mode
            .as_deref(),
        Some("auto")
    );
}
