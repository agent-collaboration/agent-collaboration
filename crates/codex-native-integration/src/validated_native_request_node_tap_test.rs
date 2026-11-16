//! N10 node taps: one request from a Codex-client user goes to Managed Codex,
//! and the reply is returned, rejected as malformed (retiring the connection),
//! or refused before dispatch.
//!
//! Catalog source: the Program Design's N10 section and R17 (malformed replies
//! keep today's outcomes: an effectful call is `OutcomeUnknown`, a read is
//! `Protocol`, and the connection is retired either way).
//!
//! Recording layers: the user's call and N10's returned `Result` are typed values
//! (`TypedInput`, `TypedOutcome`); every frame on the Managed Codex socket, both
//! directions, is a `WireFrame` taken from the fake app-server's own transcript.
use crate::{
    NativeConnectionError, NativeOperation, NativePayloadSchemas, NativeProtocolConnection,
    NativeSchemaBundle,
};
use codex_router_test_support::node_taps::{
    CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode, RecordIntent,
    RecordLayer, TapCase,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};

fn schemas() -> NativePayloadSchemas {
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
        definitions.insert(format!("{name}Params"), json!({"type":"object","required":["threadId"],"properties":{"threadId":{"type":"string"}}}));
        definitions.insert(
            format!("{name}Response"),
            json!({"type":"object","required":["ok"],"properties":{"ok":{"type":"boolean"}}}),
        );
    }
    let bundle = NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))
            .unwrap_or_else(|error| panic!("schema: {error}")),
    )]))
    .unwrap_or_else(|error| panic!("bundle: {error}"));
    NativePayloadSchemas::from_bundle(&bundle).unwrap_or_else(|error| panic!("schemas: {error}"))
}

/// One scripted reply of the fake Managed Codex and what the test means it to be.
struct ScriptedReply {
    frame: Value,
    intent: RecordIntent,
}

/// A stand-in reply shaped for the test's permissive `{ok}` schema bundle, not a real Codex
/// response: its differences from the pinned Codex export are apparatus findings.
fn stand_in_reply(result: Value) -> ScriptedReply {
    ScriptedReply {
        frame: json!({"result": result}),
        intent: RecordIntent::SyntheticApparatus,
    }
}

/// A reply the test injects to prove N10 rejects it. Against the pinned Codex export its
/// violation is the response's missing required field, `missing_property`, at the result's root.
fn malformed_reply(missing_property: &'static str) -> ScriptedReply {
    ScriptedReply {
        frame: json!({"result":{"ok":"not-a-boolean"}}),
        intent: RecordIntent::DeliberateInvalid {
            keyword: "required",
            instance_path: "",
            detail: missing_property,
        },
    }
}

/// One frame on the Managed Codex socket, in the order the fake app-server saw it.
enum CodexWireFrame {
    /// A frame N10 sent: a request.
    FromRouter(Value),
    /// A frame the fake sent back: a scripted reply, with its intent.
    ToRouter(Value, RecordIntent),
}

/// A real WebSocket pair; the fake Managed Codex answers each received request
/// with the scripted reply and returns its whole transcript, both directions, in order.
async fn connection_to_fake_codex(
    replies: Vec<ScriptedReply>,
) -> (
    NativeProtocolConnection,
    tokio::task::JoinHandle<Vec<CodexWireFrame>>,
) {
    let (client, server) = tokio::net::UnixStream::pair().unwrap_or_else(|e| panic!("pair: {e}"));
    let connection = NativeProtocolConnection::from_websocket(
        WebSocketStream::from_raw_socket(client, Role::Client, None).await,
    );
    let fixture = tokio::spawn(async move {
        let mut socket = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
        let mut transcript = Vec::new();
        for reply in replies {
            let frame = socket
                .next()
                .await
                .unwrap_or_else(|| panic!("a request"))
                .unwrap_or_else(|error| panic!("frame: {error}"));
            let request: Value =
                serde_json::from_str(frame.to_text().unwrap_or_else(|e| panic!("{e}")))
                    .unwrap_or_else(|error| panic!("JSON: {error}"));
            let mut answer = reply.frame;
            answer["id"] = request["id"].clone();
            transcript.push(CodexWireFrame::FromRouter(request));
            transcript.push(CodexWireFrame::ToRouter(answer.clone(), reply.intent));
            socket
                .send(Message::Text(answer.to_string().into()))
                .await
                .unwrap_or_else(|error| panic!("reply: {error}"));
        }
        // Drain: any further frame before the connection closes is an extra record.
        while let Some(Ok(frame)) = socket.next().await {
            if let Ok(text) = frame.to_text()
                && let Ok(request) = serde_json::from_str::<Value>(text)
            {
                transcript.push(CodexWireFrame::FromRouter(request));
            }
        }
        transcript
    });
    (connection, fixture)
}

fn new_case(case: &str, method: &str, params: &Value) -> TapCase {
    let mut tap = TapCase::new(
        ProtocolNode::N10CodexClient,
        case,
        "N10 section: request<R> -> R::Response or CodexCallError; R17 malformed replies",
    );
    tap.connection(MANAGED_CODEX_CONNECTION);
    // The user's call as handed to N10: a method and params, never a frame.
    tap.consumed_with(
        RecordLayer::TypedInput,
        MessageFamily::CodexRequestToCodex,
        EdgeParty::CodexClientUser,
        method,
        params.clone(),
    );
    tap
}

/// The one socket between N10 and the fake Managed Codex.
const MANAGED_CODEX_CONNECTION: &str = "managed-codex";

/// The id the connection gives its first request on the wire (a fresh connection counts from 0).
const FIRST_REQUEST_ID: &str = "agent-collaboration-0";

/// The thread id of the state probe that `connection_state` sends; the probe is test apparatus,
/// not N10 output under test, but its frames are on the same socket and are recorded and checked.
const PROBE_THREAD_ID: &str = "probe";

/// Splits the transcript where the state probe starts: the call under test, then the probe.
fn split_at_probe(transcript: &[CodexWireFrame]) -> (&[CodexWireFrame], &[CodexWireFrame]) {
    let probe_start = transcript
        .iter()
        .position(|frame| {
            matches!(frame, CodexWireFrame::FromRouter(request)
                if request["params"]["threadId"] == PROBE_THREAD_ID)
        })
        .unwrap_or(transcript.len());
    transcript.split_at(probe_start)
}

/// Records each frame exactly as it crossed, pairing replies with the requests before them.
fn record_wire(case: &mut TapCase, frames: &[CodexWireFrame], setup: bool) {
    let mut methods_by_id: BTreeMap<String, String> = BTreeMap::new();
    for frame in frames {
        match frame {
            CodexWireFrame::FromRouter(request) => {
                let method = request["method"].as_str().unwrap_or("").to_owned();
                methods_by_id.insert(request["id"].to_string(), method.clone());
                case.produced_raw(
                    RecordLayer::WireFrame,
                    MessageFamily::CodexRequestToCodex,
                    EdgeParty::ManagedCodex,
                    &method,
                    request.clone(),
                    &[],
                );
                if setup {
                    case.mark_last_setup();
                }
            }
            CodexWireFrame::ToRouter(reply, intent) => {
                let method = methods_by_id
                    .get(&reply["id"].to_string())
                    .map_or("unknown", String::as_str);
                let outcome = if reply.get("error").is_some() {
                    "error"
                } else {
                    "response"
                };
                case.consumed_reply_raw(
                    RecordLayer::WireFrame,
                    MessageFamily::CodexRequestToCodex,
                    EdgeParty::ManagedCodex,
                    &format!("{outcome}:{method}"),
                    reply.clone(),
                    &[],
                );
                case.mark_last(intent.clone());
                if setup {
                    case.mark_last_setup();
                }
            }
        }
    }
}

/// Table gap: the table has no row for N10's answer to its user; it is
/// recorded as a message from Codex, the closest row.
fn record_answer(case: &mut TapCase, answer: &Result<Value, NativeConnectionError>, method: &str) {
    let kind = match answer {
        Ok(_) => format!("response:{method}"),
        Err(NativeConnectionError::Rejected { code }) => format!("error:{code}:{method}"),
        Err(error) => format!("error:{}:{method}", error_name(error)),
    };
    // The whole returned response: `Ok(result)` or the whole `CodexCallError`-side variant.
    let payload = answer_payload(answer);
    case.produced_with(
        RecordLayer::TypedOutcome,
        MessageFamily::CodexMessageFromCodex,
        EdgeParty::CodexClientUser,
        &kind,
        payload,
    );
}

/// Records one case's whole socket transcript around N10's answer: the call's frames, the
/// returned answer, then the state probe's frames, marked setup (checked, not in the row).
fn record_exchange(
    case: &mut TapCase,
    transcript: &[CodexWireFrame],
    answer: &Result<Value, NativeConnectionError>,
    method: &str,
) {
    let (call, probe) = split_at_probe(transcript);
    record_wire(case, call, false);
    record_answer(case, answer, method);
    record_wire(case, probe, true);
}
fn answer_payload(answer: &Result<Value, NativeConnectionError>) -> Value {
    json!({"debug": format!("{answer:#?}")})
}

fn error_name(error: &NativeConnectionError) -> &'static str {
    match error {
        NativeConnectionError::InvalidInput => "InvalidInput",
        NativeConnectionError::Unavailable => "Unavailable",
        NativeConnectionError::UnavailableWithCause(_) => "UnavailableWithCause",
        NativeConnectionError::OutcomeUnknown => "OutcomeUnknown",
        NativeConnectionError::OutcomeUnknownWithCause(_) => "OutcomeUnknownWithCause",
        NativeConnectionError::Protocol => "Protocol",
        NativeConnectionError::Rejected { .. } => "Rejected",
    }
}

/// Whether a follow-up call on this connection is dispatched. A retired
/// connection refuses it without sending a frame.
async fn connection_state(connection: &mut NativeProtocolConnection) -> NodeStateSnapshot {
    let probe = connection
        .request("thread/read", json!({"threadId": PROBE_THREAD_ID}))
        .await;
    let state = if matches!(probe, Err(NativeConnectionError::Unavailable)) {
        "retired"
    } else {
        "usable"
    };
    NodeStateSnapshot::new().field("connection", state)
}

/// A whole `turn/start` call as a real user sends it: the thread and its text input.
fn turn_start_params() -> Value {
    json!({"threadId":"thread-a","input":[{"type":"text","text":"hello","text_elements":[]}]})
}

/// The row for one call: the frame N10 sends (if any) and its answer to the user.
fn answer_row(
    sent: Option<(&str, Value)>,
    answer: (&str, Result<Value, NativeConnectionError>),
    connection: &str,
) -> CatalogRowExpectation {
    let mut row = CatalogRowExpectation::new();
    if let Some((method, params)) = sent {
        row = row.produces_with(
            MessageFamily::CodexRequestToCodex,
            EdgeParty::ManagedCodex,
            method,
            json!({"id": FIRST_REQUEST_ID, "method": method, "params": params}),
        );
    }
    row.produces_with(
        MessageFamily::CodexMessageFromCodex,
        EdgeParty::CodexClientUser,
        answer.0,
        answer_payload(&answer.1),
    )
    .state_after(NodeStateSnapshot::new().field("connection", connection))
}

#[tokio::test]
async fn node_tap_valid_reply_returns_response_and_keeps_connection() {
    let schemas = schemas();
    // The second scripted reply answers the state probe.
    let (mut connection, fixture) = connection_to_fake_codex(vec![
        stand_in_reply(json!({"ok":true})),
        stand_in_reply(json!({"ok":true})),
    ])
    .await;
    let answer = connection
        .request_validated(
            &schemas,
            NativeOperation::ReadThread,
            json!({"threadId":"thread-a"}),
        )
        .await;
    let state = connection_state(&mut connection).await;
    drop(connection);
    let transcript = fixture.await.unwrap_or_else(|e| panic!("fixture: {e}"));
    let mut case = new_case(
        "valid_reply_returned",
        "thread/read",
        &json!({"threadId":"thread-a"}),
    );
    case.state_before(NodeStateSnapshot::new().field("connection", "usable"));
    record_exchange(&mut case, &transcript, &answer, "thread/read");
    case.state_after(state);
    case.finish(&answer_row(
        Some(("thread/read", json!({"threadId":"thread-a"}))),
        ("response:thread/read", Ok(json!({"ok":true}))),
        "usable",
    ))
    .assert_settled();
}

#[tokio::test]
async fn node_tap_malformed_reply_to_effectful_call_is_outcome_unknown_and_retires() {
    let schemas = schemas();
    let (mut connection, fixture) = connection_to_fake_codex(vec![malformed_reply("turn")]).await;
    let answer = connection
        .request_validated(&schemas, NativeOperation::StartTurn, turn_start_params())
        .await;
    let state = connection_state(&mut connection).await;
    drop(connection);
    let transcript = fixture.await.unwrap_or_else(|e| panic!("fixture: {e}"));
    let mut case = new_case(
        "malformed_effectful_reply_retires",
        "turn/start",
        &turn_start_params(),
    );
    case.state_before(NodeStateSnapshot::new().field("connection", "usable"));
    record_exchange(&mut case, &transcript, &answer, "turn/start");
    case.state_after(state);
    case.finish(&answer_row(
        Some(("turn/start", turn_start_params())),
        (
            "error:OutcomeUnknown:turn/start",
            Err(NativeConnectionError::OutcomeUnknown),
        ),
        "retired",
    ))
    .assert_settled();
}

#[tokio::test]
async fn node_tap_malformed_reply_to_read_is_protocol_and_retires() {
    let schemas = schemas();
    let (mut connection, fixture) = connection_to_fake_codex(vec![malformed_reply("thread")]).await;
    let answer = connection
        .request_validated(
            &schemas,
            NativeOperation::ReadThread,
            json!({"threadId":"thread-a"}),
        )
        .await;
    let state = connection_state(&mut connection).await;
    drop(connection);
    let transcript = fixture.await.unwrap_or_else(|e| panic!("fixture: {e}"));
    let mut case = new_case(
        "malformed_read_reply_retires",
        "thread/read",
        &json!({"threadId":"thread-a"}),
    );
    case.state_before(NodeStateSnapshot::new().field("connection", "usable"));
    record_exchange(&mut case, &transcript, &answer, "thread/read");
    case.state_after(state);
    case.finish(&answer_row(
        Some(("thread/read", json!({"threadId":"thread-a"}))),
        (
            "error:Protocol:thread/read",
            Err(NativeConnectionError::Protocol),
        ),
        "retired",
    ))
    .assert_settled();
}

#[tokio::test]
async fn node_tap_invalid_request_is_refused_before_dispatch() {
    let schemas = schemas();
    // One scripted reply answers the state probe; the invalid call sends nothing.
    let (mut connection, fixture) =
        connection_to_fake_codex(vec![stand_in_reply(json!({"ok":true}))]).await;
    let answer = connection
        .request_validated(&schemas, NativeOperation::StartTurn, json!({"threadId":12}))
        .await;
    let state = connection_state(&mut connection).await;
    drop(connection);
    let transcript = fixture.await.unwrap_or_else(|e| panic!("fixture: {e}"));
    let mut case = new_case(
        "invalid_request_not_dispatched",
        "turn/start",
        &json!({"threadId":12}),
    );
    case.state_before(NodeStateSnapshot::new().field("connection", "usable"));
    record_exchange(&mut case, &transcript, &answer, "turn/start");
    case.state_after(state);
    case.finish(&answer_row(
        None,
        (
            "error:InvalidInput:turn/start",
            Err(NativeConnectionError::InvalidInput),
        ),
        "usable",
    ))
    .assert_settled();
}

#[tokio::test]
async fn node_tap_error_reply_is_rejected_and_keeps_connection() {
    let schemas = schemas();
    let (mut connection, fixture) = connection_to_fake_codex(vec![
        ScriptedReply {
            frame: json!({"error":{"code":-32600,"message":"busy"}}),
            intent: RecordIntent::Product,
        },
        stand_in_reply(json!({"ok":true})),
    ])
    .await;
    let answer = connection
        .request_validated(&schemas, NativeOperation::StartTurn, turn_start_params())
        .await;
    let state = connection_state(&mut connection).await;
    drop(connection);
    let transcript = fixture.await.unwrap_or_else(|e| panic!("fixture: {e}"));
    let mut case = new_case("error_reply_rejected", "turn/start", &turn_start_params());
    case.state_before(NodeStateSnapshot::new().field("connection", "usable"));
    record_exchange(&mut case, &transcript, &answer, "turn/start");
    case.state_after(state);
    case.finish(&answer_row(
        Some(("turn/start", turn_start_params())),
        (
            "error:-32600:turn/start",
            Err(NativeConnectionError::Rejected { code: -32600 }),
        ),
        "usable",
    ))
    .assert_settled();
}
