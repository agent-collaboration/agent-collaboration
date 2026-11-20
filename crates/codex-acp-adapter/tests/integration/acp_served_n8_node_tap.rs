//! N8 node tap on its real ACP wire: one `session/prompt` on the public connection becomes
//! Codex calls, a Codex message becomes an ACP update, and the turn's completion becomes the
//! prompt's response.
//!
//! The case drives `serve_acp_connection` over a Unix socket pair and records every line on the
//! test's end of it, both directions, from the connection's first frame: the ACP edge as it
//! crossed (`WireFrame`). The adapter's backend connection reaches a fake Codex app-server on a
//! listening socket, whose transcript is recorded the same way. Session setup (`initialize`,
//! `session/load`, and on the Codex socket `initialize`, `initialized`, `thread/resume`) is
//! marked setup: checked and counted, but not part of the row.
use codex_acp_adapter::{
    AcpConnectionInputs, AcpSessionBinding, AcpStoredSessions, HeldBindingCheckout,
    UnmaterializedBindingStore, serve_acp_connection,
};
use codex_native_integration::{NativePayloadSchemas, NativeSchemaBundle};
use codex_router_test_support::node_taps::{
    CatalogRowExpectation, EdgeParty, MessageFamily, ProtocolNode, RecordLayer, TapCase,
};
use fake_codex::{CodexTranscript, CodexWireRecorder, Step, spawn_fake_codex};
use serde_json::{Value, json};
use std::{collections::BTreeMap, future::Future, io, pin::Pin, sync::Arc};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

#[path = "../../src/native_prompt_execution_node_tap_fake_codex.rs"]
mod fake_codex;

/// The ACP connection N8 serves, between the test (its client) and `serve_acp_connection`.
const ACP_CARRIER_CONNECTION: &str = "acp-carrier";

/// Codex setup before the case's own calls: `initialize`, `initialized`, `thread/resume`.
const LOADED_SESSION_SETUP_REQUESTS: usize = 3;

struct NoHeldBindings;
impl UnmaterializedBindingStore for NoHeldBindings {
    fn hold(&self, _binding: AcpSessionBinding) {}
    fn checkout(&self, _session_id: &str) -> HeldBindingCheckout {
        HeldBindingCheckout::Missing
    }
    fn restore(&self, _binding: AcpSessionBinding) {}
    fn finish(&self, _session_id: &str) {}
    fn host_tasks(&self) -> tokio_util::task::TaskTracker {
        tokio_util::task::TaskTracker::new()
    }
}

struct NoStoredSessions;
impl AcpStoredSessions for NoStoredSessions {
    fn list(&self, _params: Value) -> Pin<Box<dyn Future<Output = io::Result<Value>> + Send + '_>> {
        Box::pin(async { Ok(json!({"sessions":[]})) })
    }
}

fn native_schemas() -> Arc<NativePayloadSchemas> {
    let mut definitions = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
    ] {
        definitions.insert(format!("{name}Params"), json!({"type":"object"}));
        definitions.insert(format!("{name}Response"), json!({"type":"object"}));
    }
    let bundle = NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{
            "v2":definitions,"ServerRequest":{"type":"object"},"ServerNotification":{"type":"object"}
        }}))
        .unwrap_or_else(|error| panic!("schema JSON: {error}")),
    )]))
    .unwrap_or_else(|error| panic!("bundle: {error}"));
    Arc::new(
        NativePayloadSchemas::from_bundle(&bundle)
            .unwrap_or_else(|error| panic!("schemas: {error}")),
    )
}

/// The test's end of the ACP socket, recording each line it writes or reads as it crossed.
struct AcpClient {
    read: BufReader<OwnedReadHalf>,
    write: OwnedWriteHalf,
    /// Each setup request's id, with its method, so its reply is named by what it answers.
    setup_requests: Vec<(Value, String)>,
}

impl AcpClient {
    async fn send(&mut self, case: &mut TapCase, frame: Value, setup: bool) {
        let method = frame["method"].as_str().unwrap_or("unknown").to_owned();
        if setup {
            self.setup_requests
                .push((frame["id"].clone(), method.clone()));
        }
        self.write
            .write_all(format!("{frame}\n").as_bytes())
            .await
            .unwrap_or_else(|error| panic!("write {method}: {error}"));
        case.connection(ACP_CARRIER_CONNECTION);
        case.consumed_raw(
            RecordLayer::WireFrame,
            MessageFamily::AcpRequestToCodexAdapter,
            EdgeParty::McpCaller,
            &method,
            frame,
            &[],
        );
        if setup {
            case.mark_last_setup();
        }
    }

    /// Reads and records one frame from the adapter.
    async fn receive(&mut self, case: &mut TapCase) -> Value {
        let frame = read_frame(&mut self.read)
            .await
            .unwrap_or_else(|| panic!("the adapter closed the ACP connection"));
        self.record_received(case, &frame);
        frame
    }

    /// Reads and records every frame the adapter sends until it closes the connection, and
    /// returns them: the capture is complete only once the reader reaches end of stream.
    async fn drain_until_close(&mut self, case: &mut TapCase) -> Vec<Value> {
        let frames = read_until_close(&mut self.read).await;
        for frame in &frames {
            self.record_received(case, frame);
        }
        frames
    }

    fn record_received(&self, case: &mut TapCase, frame: &Value) {
        case.connection(ACP_CARRIER_CONNECTION);
        let family = MessageFamily::AcpServedForCodexSessions;
        let counterpart = EdgeParty::McpCaller;
        if let Some(method) = frame["method"].as_str() {
            let kind = match frame["params"]["update"]["sessionUpdate"].as_str() {
                Some(update) => format!("{method}:{update}"),
                None => method.to_owned(),
            };
            case.produced_raw(
                RecordLayer::WireFrame,
                family,
                counterpart,
                &kind,
                frame.clone(),
                &[],
            );
        } else if let Some((_, answered)) = self
            .setup_requests
            .iter()
            .find(|(id, _)| id == &frame["id"])
        {
            case.produced_reply_raw(
                RecordLayer::WireFrame,
                MessageFamily::AcpRequestToCodexAdapter,
                counterpart,
                &format!("response:{answered}"),
                frame.clone(),
                &[],
            );
            case.mark_last_setup();
        } else {
            // The prompt's response is the "ACP served for Codex sessions" message N8 produces
            // for its client (the table's row for it), as in the unit cases.
            case.produced_raw(
                RecordLayer::WireFrame,
                family,
                counterpart,
                &format!(
                    "response:session/prompt stopReason={}",
                    frame["result"]["stopReason"].as_str().unwrap_or("none")
                ),
                frame.clone(),
                &[],
            );
        }
    }
}

/// One newline-delimited frame from the adapter, or `None` at end of stream.
async fn read_frame(read: &mut BufReader<OwnedReadHalf>) -> Option<Value> {
    let mut line = String::new();
    let read = read
        .read_line(&mut line)
        .await
        .unwrap_or_else(|error| panic!("read: {error}"));
    (read > 0)
        .then(|| serde_json::from_str(&line).unwrap_or_else(|error| panic!("frame JSON: {error}")))
}

/// Every frame left on the connection, read until the peer closes it.
async fn read_until_close(read: &mut BufReader<OwnedReadHalf>) -> Vec<Value> {
    let mut frames = Vec::new();
    while let Some(frame) = read_frame(read).await {
        frames.push(frame);
    }
    frames
}

fn n10_call(
    expectation: CatalogRowExpectation,
    method: &str,
    id: &str,
    params: Value,
) -> CatalogRowExpectation {
    expectation.produces_with(
        MessageFamily::CodexRequestToCodex,
        EdgeParty::Node(ProtocolNode::N10CodexClient),
        method,
        json!({"id": id, "method": method, "params": params}),
    )
}

#[tokio::test]
async fn node_tap_acp_prompt_on_the_public_connection_becomes_codex_calls_update_and_response() {
    // Arrange: a fake Codex behind the adapter's backend socket, and the public ACP connection.
    let socket = std::env::temp_dir().join(format!("acp-served-n8-{}.sock", std::process::id()));
    let _stale = std::fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket)
        .unwrap_or_else(|error| panic!("backend bind: {error}"));
    let transcript = CodexTranscript::default();
    let script = vec![
        Step::Request("initialize", json!({})),
        Step::Request("initialized", Value::Null),
        Step::Request(
            "thread/resume",
            json!({"cwd":"/work","thread":{"id":"thread-a","cwd":"/work","status":{"type":"idle"},"turns":[]}}),
        ),
        Step::Request(
            "thread/read",
            json!({"thread":{"id":"thread-a","status":{"type":"idle"},"turns":[]}}),
        ),
        Step::Request("turn/start", json!({"turn":{"id":"turn-a"}})),
        Step::Send(
            json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-a","turnId":"turn-a","itemId":"message-a","delta":"hi there"}}),
        ),
        Step::Send(
            json!({"method":"turn/completed","params":{"threadId":"thread-a","turn":{"id":"turn-a","items":[],"status":"completed"}}}),
        ),
        Step::Request(
            "thread/read",
            json!({"thread":{"id":"thread-a","model":"gpt-5.6-sol","reasoningEffort":"medium","createdAt":1}}),
        ),
    ];
    let backend_transcript = transcript.clone();
    let backend = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .unwrap_or_else(|error| panic!("backend accept: {error}"));
        let wire = tokio_tungstenite::accept_async(stream)
            .await
            .unwrap_or_else(|error| panic!("upgrade: {error}"));
        spawn_fake_codex(wire, script, backend_transcript)
            .await
            .unwrap_or_else(|error| panic!("fake Codex: {error}"));
    });
    let (client, server) =
        tokio::net::UnixStream::pair().unwrap_or_else(|error| panic!("pair: {error}"));
    let serving = tokio::spawn(serve_acp_connection(
        server,
        AcpConnectionInputs {
            scratch_scopes: Arc::new(
                crate::recording_scratch_scopes::RecordingScratchScopes::default(),
            ),
            backend_path: socket.clone(),
            generation: serde_json::from_value(
                json!({"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1}),
            )
            .unwrap_or_else(|error| panic!("generation: {error}")),
            schemas: native_schemas(),
            stored_sessions: Arc::new(NoStoredSessions),
            holder: Arc::new(NoHeldBindings),
            recorder: Arc::new(
                crate::conversation_operation_recorder::AcceptingConversationRecorder,
            ),
            retired: tokio_util::sync::CancellationToken::new(),
        },
    ));
    let (read, write) = client.into_split();
    let mut acp = AcpClient {
        read: BufReader::new(read),
        write,
        setup_requests: Vec::new(),
    };
    let mut case = TapCase::new(
        ProtocolNode::N8CodexAcpAdapter,
        "acp_prompt_on_public_connection",
        "N8 section: ACP prompt from the carrier -> Codex calls to N10 -> ACP updates and the terminal response to the carrier",
    );
    // Router-owned `_meta` is held to the shared owning checks (initialize's profile metadata).
    case.use_owned_codecs(
        codex_router_test_support::node_taps::OwnedCodecs::from_owner_registry(
            acp_provider_adapters::owned_contract_checks::round_trips(),
            acp_provider_adapters::owned_contract_checks::verdicts(),
        )
        .unwrap_or_else(|error| panic!("owner registry: {error}")),
    );
    // Keys no owning type defines yet, named instead of passed (correction 3, item B).
    case.declare_coverage_gap(
        "session/load",
        "N8's session/load _meta key codex-router/replayHistory has no owning type: collaboration-client writes it by hand (acp_session_setup.rs:89) and N8 reads it by hand (history_projection.rs:9)",
    );
    case.declare_coverage_gap(
        "response:session/prompt stopReason=end_turn",
        "N8's prompt result _meta.codexRouter and codex-router/finalReply have no owning type: N8 builds them by hand (native_prompt_execution.rs:371, 448) and collaboration-client reads them by hand (acp_conversation.rs:587)",
    );

    // Act: initialize, load the session, then prompt and read until the prompt's response.
    acp.send(
        &mut case,
        json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":1}}),
        true,
    )
    .await;
    acp.receive(&mut case).await;
    acp.send(
        &mut case,
        json!({"jsonrpc":"2.0","id":"load","method":"session/load","params":{"sessionId":"thread-a","cwd":"/work","mcpServers":[],"_meta":{"codex-router/replayHistory":false}}}),
        true,
    )
    .await;
    acp.receive(&mut case).await;
    acp.send(
        &mut case,
        json!({"jsonrpc":"2.0","id":"acp-prompt","method":"session/prompt","params":{"sessionId":"thread-a","prompt":[{"type":"text","text":"hello"}]}}),
        false,
    )
    .await;
    while acp.receive(&mut case).await["id"] != "acp-prompt" {}
    // Close the client's side, then read until the adapter closes the connection: every frame it
    // sends after the terminal response is recorded too, so the capture is complete to the close.
    acp.write
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));
    let after_terminal = acp.drain_until_close(&mut case).await;
    serving
        .await
        .unwrap_or_else(|error| panic!("join: {error}"))
        .unwrap_or_else(|error| panic!("serve: {error}"));
    backend
        .await
        .unwrap_or_else(|error| panic!("backend: {error}"));
    CodexWireRecorder::with_setup_requests(LOADED_SESSION_SETUP_REQUESTS)
        .record_new(&mut case, &transcript);
    let _removed = std::fs::remove_file(&socket);

    // Assert: the case's own messages match the N8 row, and every recorded frame is settled.
    // The backend connection's own `initialize` is request 0 (native_protocol_connection.rs:100)
    // and `thread/resume` is 1, so the case's calls are 2-4. The final reply is selected only from
    // a completed item (final_reply_selection.rs:34); the streamed delta is not one, so it is null.
    // Records are in recording order: the ACP frames as they crossed, then the Codex transcript.
    let acp_row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::AcpServedForCodexSessions,
            EdgeParty::McpCaller,
            "session/update:agent_message_chunk",
            json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"thread-a","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi there"}}}}),
        )
        .produces_with(
            MessageFamily::AcpServedForCodexSessions,
            EdgeParty::McpCaller,
            "response:session/prompt stopReason=end_turn",
            json!({"jsonrpc":"2.0","id":"acp-prompt","result":{"stopReason":"end_turn","_meta":{"codex-router/finalReply":{"kind":"available","text":null},"codexRouter":{"effectiveModel":"gpt-5.6-sol","effectiveEffort":"medium","effectiveAccess":null,"settingsObservation":{"kind":"unavailable","reason":"noRecordedAccessRoute"},"idleSeconds":0}}}}),
        );
    let read_thread = json!({"threadId":"thread-a","includeTurns":false});
    let row = n10_call(
        n10_call(
            n10_call(
                acp_row,
                "thread/read",
                "agent-collaboration-2",
                read_thread.clone(),
            ),
            "turn/start",
            "agent-collaboration-3",
            json!({"threadId":"thread-a","input":[{"type":"text","text":"hello","text_elements":[]}]}),
        ),
        "thread/read",
        "agent-collaboration-4",
        read_thread,
    );
    assert!(
        after_terminal.is_empty(),
        "frames after the terminal response: {after_terminal:?}"
    );
    case.finish(&row).assert_settled();
}

/// The drain's sensitivity control: a frame the peer writes after the terminal response is read
/// and returned, not lost when the connection closes.
#[tokio::test]
async fn the_drain_returns_a_frame_written_after_the_terminal_response() {
    // Arrange: a peer that writes a terminal response and one more frame, then closes.
    let (client, mut peer) =
        tokio::net::UnixStream::pair().unwrap_or_else(|error| panic!("pair: {error}"));
    let terminal = json!({"jsonrpc":"2.0","id":"acp-prompt","result":{"stopReason":"end_turn"}});
    let extra = json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"thread-a","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"late"}}}});
    peer.write_all(format!("{terminal}\n{extra}\n").as_bytes())
        .await
        .unwrap_or_else(|error| panic!("peer write: {error}"));
    drop(peer);
    let (read, _write) = client.into_split();
    let mut read = BufReader::new(read);

    // Act: read the terminal response, then drain to the close.
    let first = read_frame(&mut read).await;
    let after_terminal = read_until_close(&mut read).await;

    // Assert: the late frame is caught.
    assert_eq!(first, Some(terminal));
    assert_eq!(after_terminal, vec![extra]);
}
