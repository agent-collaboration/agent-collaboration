//! The Router's ACP socket through the real Host (D1 R12, V22): a
//! `session/prompt` for a session that is in a turn is never refused as busy.
//! With an agent that cannot steer it waits in the session's line, and its
//! response arrives when the turn it started ends.

#![expect(
    clippy::expect_used,
    reason = "A Host or fixture that cannot start or answer must fail its test."
)]

use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

/// Every ACP request the fixture recorded with `method`.
fn recorded(receipt: &Path, method: &str) -> usize {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .map_or(0, |requests| {
            requests
                .iter()
                .filter(|request| request["method"] == method)
                .count()
        })
}

/// Waits, bounded and without sleeping, until the fixture recorded `count`.
/// Waits until the fixture recorded `count`; the test runner bounds the case
/// (`.config/nextest.toml`), so test code never waits on Tokio's clock.
async fn until_recorded(receipt: &Path, method: &str, count: usize) {
    while recorded(receipt, method) < count {
        tokio::task::yield_now().await;
    }
}

/// The next response frame (a frame with a result or error), skipping notifications.
async fn next_response(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
) -> Value {
    loop {
        let line = lines
            .next_line()
            .await
            .expect("ACP socket read")
            .expect("ACP socket stays open");
        let frame: Value = serde_json::from_str(&line).expect("ACP frame is JSON");
        if frame.get("result").is_some() || frame.get("error").is_some() {
            return frame;
        }
    }
}

async fn send(write: &mut tokio::net::unix::OwnedWriteHalf, request: &Value) {
    write
        .write_all(format!("{request}\n").as_bytes())
        .await
        .expect("ACP request");
}

/// Oracle: D1 R12 and V22 — on the Router's ACP socket the second prompt to a
/// busy, non-steering session waits in the line: the agent sees no second
/// prompt until the first turn ends, then exactly one; both prompts answer and
/// nothing is cancelled.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acp_busy_prompt_through_the_host_command_port() {
    // Arrange: a Host with the Cursor fixture that holds its first prompt's turn.
    let root = tempfile::tempdir().expect("runtime root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private runtime root");
    let receipt = root.path().join("wire.json");
    let binding = ExternalProviderLaunchBinding::cursor(
        node_test_fixture::node_executable().expect("Node"),
        vec![
            node_test_fixture::fixture_program("cursor-settings-agent.ts")
                .to_string_lossy()
                .into_owned(),
            "reactive".to_owned(),
            receipt.display().to_string(),
            "hold-first-prompt".to_owned(),
        ],
    )
    .expect("Cursor binding");
    let runtime = Box::pin(CollaborationRuntime::start_with_external_providers(
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
        vec![ExternalProviderStartup::Launch(binding)],
    ))
    .await
    .expect("Host starts the provider face");
    let stream = UnixStream::connect(root.path().join("codex-acp.sock"))
        .await
        .expect("ACP socket");
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    send(
        &mut write,
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":1,"clientCapabilities":{},"_meta":{
                "router":{"actor":{"kind":"human","humanId":"acp-owner"}},
                "sessionProfile":{"version":1,"elements":[]}
            }
        }}),
    )
    .await;
    assert!(
        next_response(&mut lines).await.get("error").is_none(),
        "initialize"
    );
    send(
        &mut write,
        &json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{
            "cwd":root.path(),"mcpServers":[],"_meta":{"router":{"endpoint":"cursor-local"}}
        }}),
    )
    .await;
    let created = next_response(&mut lines).await;
    assert!(created.get("error").is_none(), "{created}");
    let session_id = created["result"]["sessionId"]
        .as_str()
        .expect("session id")
        .to_owned();
    let prompt = |id: u64, text: &str| {
        json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{
            "sessionId":session_id,"prompt":[{"type":"text","text":text}]
        }})
    };

    // Act: prompt 1 is held; prompt 2 arrives while it runs; then the turn is released.
    send(&mut write, &prompt(3, "first")).await;
    until_recorded(&receipt, "session/prompt", 1).await;
    send(&mut write, &prompt(4, "second")).await;
    let prompts_while_held = recorded(&receipt, "session/prompt");
    std::fs::write(root.path().join("wire.json.release"), b"").expect("release the held turn");
    let first = next_response(&mut lines).await;
    let second = next_response(&mut lines).await;
    until_recorded(&receipt, "session/prompt", 2).await;
    let prompts_after = recorded(&receipt, "session/prompt");
    let cancels = recorded(&receipt, "session/cancel");
    runtime.shutdown().await.expect("Host shutdown");

    // Assert: both answered in order, the agent saw one prompt while held then
    // exactly two, and the Router cancelled nothing.
    assert_eq!(first["id"], 3, "{first}");
    assert_eq!(first["result"]["stopReason"], "end_turn", "{first}");
    assert_eq!(second["id"], 4, "{second}");
    assert_eq!(second["result"]["stopReason"], "end_turn", "{second}");
    assert_eq!(prompts_while_held, 1);
    assert_eq!(prompts_after, 2);
    assert_eq!(cancels, 0);
}
