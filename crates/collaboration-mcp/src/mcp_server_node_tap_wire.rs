//! What N11's taps see on their wires: the MCP caller's HTTP exchanges with the served API, the
//! fake Codex ACP adapter's socket, and the fake Codex app-server's socket.
//!
//! Each records whole frames exactly as they crossed, both directions, from a connection's
//! first frame on: the MCP `initialize` and `tools/list` exchanges a call is checked against,
//! the ACP `initialize`, and N10's Codex handshake. The MCP handshake and N10's Codex handshake are
//! marked setup: recorded and checked, but not part of the row.
use crate::api_test_harness::{ServedApi, TEST_PROTOCOL_VERSION};
use codex_router_test_support::node_taps::{
    EdgeParty, MessageFamily, ProtocolNode, RecordIntent, RecordLayer, TapCase,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// The MCP caller's HTTP client session with the served API.
pub(super) const MCP_CALLER_CONNECTION: &str = "mcp-caller";
/// N11's ACP connection to the fake Codex ACP adapter.
pub(super) const ACP_ADAPTER_CONNECTION: &str = "acp-adapter";
/// N10's socket to the fake Codex app-server.
pub(super) const MANAGED_CODEX_CONNECTION: &str = "managed-codex";

/// Posts one handshake request and records it and its whole reply on the MCP caller's
/// connection, both marked setup.
/// How the server's answer to a handshake request is filed in the producer/consumer table.
enum AnswerRow {
    /// A reply on the request's own row.
    Reply,
    /// The advertised tool schemas: the "MCP tool schema" row N11 produces for its callers.
    ToolSchema,
}

async fn exchange(
    listener: &ServedApi,
    case: &mut TapCase,
    family: MessageFamily,
    answer_row: AnswerRow,
    kind: &str,
    request: &Value,
) -> Value {
    case.connection(MCP_CALLER_CONNECTION);
    case.consumed_raw(
        RecordLayer::WireFrame,
        family,
        EdgeParty::McpCaller,
        kind,
        request.clone(),
        &[],
    );
    case.mark_last_setup();
    let (_, answer) = listener.post(request).await;
    match answer_row {
        AnswerRow::Reply => case.produced_reply_raw(
            RecordLayer::WireFrame,
            family,
            EdgeParty::McpCaller,
            kind,
            answer.clone(),
            &[],
        ),
        AnswerRow::ToolSchema => case.produced_raw(
            RecordLayer::WireFrame,
            MessageFamily::McpToolSchema,
            EdgeParty::McpCaller,
            kind,
            answer.clone(),
            &[],
        ),
    }
    case.mark_last_setup();
    answer
}

/// The MCP handshake a caller makes before a tool call: `initialize`, `initialized`, and the
/// `tools/list` whose advertised schemas the call is checked against. The server's whole answers
/// are recorded.
pub(super) async fn record_mcp_handshake(listener: &ServedApi, case: &mut TapCase) {
    exchange(
        listener,
        case,
        MessageFamily::McpToolCall,
        AnswerRow::Reply,
        "initialize",
        &json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
            "protocolVersion":TEST_PROTOCOL_VERSION,"capabilities":{},
            "clientInfo":{"name":"n11-node-tap","version":"0.0.0"}
        }}),
    )
    .await;
    let initialized = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
    case.consumed_raw(
        RecordLayer::WireFrame,
        MessageFamily::McpToolCall,
        EdgeParty::McpCaller,
        "initialized",
        initialized.clone(),
        &[],
    );
    case.mark_last_setup();
    let (status, _) = listener.post(&initialized).await;
    assert!(status.is_success(), "initialized notification: {status}");
    exchange(
        listener,
        case,
        MessageFamily::McpToolCall,
        AnswerRow::ToolSchema,
        "tools/list",
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    )
    .await;
}

/// Records a tool call request as the caller posts it; its reply is recorded after what the call
/// caused elsewhere.
pub(super) fn record_tool_call_request(case: &mut TapCase, request: &Value) {
    case.connection(MCP_CALLER_CONNECTION);
    case.consumed_raw(
        RecordLayer::WireFrame,
        MessageFamily::McpToolCall,
        EdgeParty::McpCaller,
        &tool_call_kind(request),
        request.clone(),
        &[],
    );
}

/// Records the whole reply to `request`, taken from the HTTP response.
pub(super) fn record_tool_call_reply(case: &mut TapCase, request: &Value, reply: &Value) {
    case.connection(MCP_CALLER_CONNECTION);
    case.produced_reply_raw(
        RecordLayer::WireFrame,
        MessageFamily::McpToolCall,
        EdgeParty::McpCaller,
        &tool_call_kind(request),
        reply.clone(),
        &[],
    );
}

fn tool_call_kind(request: &Value) -> String {
    format!(
        "tools/call:{}",
        request["params"]["name"].as_str().unwrap_or("")
    )
}

/// One frame on a fake peer's connection, in the order the peer saw it.
pub(super) enum PeerFrame {
    /// A frame the Router sent.
    FromRouter(Value),
    /// A frame the fake peer sent back.
    ToRouter(Value),
}

/// The fake Codex ACP adapter: answers `initialize`, refuses everything else, and returns its
/// whole transcript.
pub(super) fn spawn_fake_acp_adapter(
    acp: tokio::net::UnixListener,
) -> tokio::task::JoinHandle<Vec<PeerFrame>> {
    tokio::spawn(async move {
        let mut transcript = Vec::new();
        let (stream, _) = acp.accept().await.expect("ACP accept");
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        while let Some(line) = lines.next_line().await.expect("ACP read") {
            let frame: Value = serde_json::from_str(&line).expect("ACP JSON");
            let answer = match frame["method"].as_str() {
                Some("initialize") => {
                    json!({"jsonrpc":"2.0","id":frame["id"],"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true},"authMethods":[]}})
                }
                _ => {
                    json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32050,"message":"fixture load rejected","data":{"kind":"nativeRejected","stage":"load","reason":"fixture-policy"}}})
                }
            };
            transcript.push(PeerFrame::FromRouter(frame));
            transcript.push(PeerFrame::ToRouter(answer.clone()));
            writer
                .write_all(format!("{answer}\n").as_bytes())
                .await
                .expect("ACP answer");
        }
        transcript
    })
}

/// Records the fake ACP adapter's transcript: N11 is the ACP client of N8's served surface, so
/// its requests are produced toward N8 and N8's answers are consumed replies on that row.
pub(super) fn record_acp_adapter_wire(case: &mut TapCase, transcript: &[PeerFrame]) {
    case.connection(ACP_ADAPTER_CONNECTION);
    let mut methods = std::collections::BTreeMap::new();
    for frame in transcript {
        match frame {
            PeerFrame::FromRouter(request) => {
                let method = request["method"].as_str().unwrap_or("").to_owned();
                methods.insert(request["id"].to_string(), method.clone());
                // The client's version string is the build's; the request id is generated per
                // run. Both are masked in the compared copy only.
                case.produced_raw(
                    RecordLayer::WireFrame,
                    MessageFamily::AcpRequestToCodexAdapter,
                    EdgeParty::Node(ProtocolNode::N8CodexAcpAdapter),
                    &method,
                    request.clone(),
                    &["version", "id"],
                );
            }
            PeerFrame::ToRouter(reply) => {
                let method = methods
                    .get(&reply["id"].to_string())
                    .cloned()
                    .unwrap_or_default();
                case.consumed_reply_raw(
                    RecordLayer::WireFrame,
                    MessageFamily::AcpRequestToCodexAdapter,
                    EdgeParty::Node(ProtocolNode::N8CodexAcpAdapter),
                    &format!("response:{method}"),
                    reply.clone(),
                    &["id"],
                );
            }
        }
    }
}

/// The fake Codex app-server: answers each request with a stand-in result and returns its whole
/// transcript.
pub(super) fn spawn_fake_codex(
    native_listener: tokio::net::UnixListener,
) -> tokio::task::JoinHandle<Vec<PeerFrame>> {
    tokio::spawn(async move {
        let mut transcript = Vec::new();
        let (stream, _) = native_listener.accept().await.expect("native accept");
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("native websocket");
        while let Some(Ok(frame)) = socket.next().await {
            let Ok(text) = frame.to_text() else { break };
            let request: Value = serde_json::from_str(text).expect("native JSON");
            let is_initialize = request["method"] == "initialize";
            let has_id = request.get("id").is_some();
            let id = request["id"].clone();
            transcript.push(PeerFrame::FromRouter(request));
            if !has_id {
                continue;
            }
            let result = if is_initialize {
                json!({"userAgent":"fixture"})
            } else {
                json!({})
            };
            let reply = json!({"id":id,"result":result});
            transcript.push(PeerFrame::ToRouter(reply.clone()));
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    reply.to_string().into(),
                ))
                .await
                .expect("native reply");
        }
        transcript
    })
}

/// Records the fake Codex app-server's transcript, N10's `initialize` and `initialized`
/// included (marked setup). The fake's results are minimal stand-ins, not real Codex
/// responses: apparatus.
pub(super) fn record_managed_codex_wire(case: &mut TapCase, transcript: &[PeerFrame]) {
    case.connection(MANAGED_CODEX_CONNECTION);
    let mut methods = std::collections::BTreeMap::new();
    // N10's connect handshake: setup, checked but not part of the row.
    let is_handshake = |method: &str| matches!(method, "initialize" | "initialized");
    for frame in transcript {
        match frame {
            PeerFrame::FromRouter(request) => {
                let method = request["method"].as_str().unwrap_or("").to_owned();
                if let Some(id) = request.get("id") {
                    methods.insert(id.to_string(), method.clone());
                }
                case.produced_raw(
                    RecordLayer::WireFrame,
                    MessageFamily::CodexRequestToCodex,
                    EdgeParty::Node(ProtocolNode::N10CodexClient),
                    &method,
                    request.clone(),
                    &[],
                );
                if is_handshake(&method) {
                    case.mark_last_setup();
                }
            }
            PeerFrame::ToRouter(reply) => {
                let method = methods
                    .get(&reply["id"].to_string())
                    .cloned()
                    .unwrap_or_default();
                case.consumed_reply_raw(
                    RecordLayer::WireFrame,
                    MessageFamily::CodexRequestToCodex,
                    EdgeParty::Node(ProtocolNode::N10CodexClient),
                    &format!("response:{method}"),
                    reply.clone(),
                    &[],
                );
                case.mark_last(RecordIntent::SyntheticApparatus);
                if is_handshake(&method) {
                    case.mark_last_setup();
                }
            }
        }
    }
}
