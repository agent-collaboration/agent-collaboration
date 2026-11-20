//! N8 node taps: one ACP prompt becomes Codex calls, and one Codex message
//! becomes an ACP update, driven through the real `PendingAcpPrompt`.
//!
//! Every produced record comes from what the fake Codex app-server saw on its
//! socket or what the adapter returned; the catalog expectations are written from
//! the Program Design's N8 section and its producer/consumer table.
//!
//! Recording layers: every frame on the Codex socket, both directions and from the
//! binding's first `thread/resume` on, is a `WireFrame` (see `fake_codex`). The ACP
//! prompt params are a `Body` (the adapter is handed them without their envelope).
//! Every `PromptEvent` `next_event` returns, the ACP updates and the terminal response
//! included, is a Rust API result (`TypedOutcome`): these cases never run the ACP
//! carrier's writer, so nothing here is an ACP wire frame. N8's ACP wire is recorded by
//! the integration case `acp_served_n8_node_tap`, at the public connection's socket.
//! The session setup frames that precede the case's own output on the same socket are marked
//! setup: checked and counted, but not part of the row.
use super::{PendingAcpPrompt, PromptEvent};
use crate::{
    AcpSchemaCatalog, AcpSessionBinding, ConversationOperationRecorder, ConversationRecordFuture,
    ScratchScopeRegister, SessionSetupInputs,
};
use codex_native_integration::{
    NativePayloadSchemas, NativeProtocolConnection, NativeSchemaBundle,
};
use codex_router_test_support::node_taps::{
    BodyRoot, CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode,
    RecordLayer, TapCase,
};
use collaboration_protocol::{CodexGeneration, OperationId, SessionId};
use fake_codex::{CodexTranscript, CodexWireRecorder, Step, spawn_fake_codex};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};

#[path = "native_prompt_execution_node_tap_fake_codex.rs"]
mod fake_codex;

const PROMPT_ID: &str = "acp-prompt";

/// The ACP carrier connection N8 serves its prompt on.
const ACP_CARRIER_CONNECTION: &str = "acp-carrier";

struct IgnoredScratchScopes;
impl ScratchScopeRegister for IgnoredScratchScopes {
    fn opened(&self, _: &std::path::Path) {}
    fn closed(&self, _: &std::path::Path) {}
}

struct AcceptingRecorder;
impl ConversationOperationRecorder for AcceptingRecorder {
    fn admit_create<'a>(
        &'a self,
        _: &'a OperationId,
        _: &'a CodexGeneration,
    ) -> ConversationRecordFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn before_native_dispatch<'a>(&'a self, _: &'a OperationId) -> ConversationRecordFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn record_created<'a>(
        &'a self,
        _: &'a OperationId,
        _: &'a SessionId,
        _: &'a collaboration_protocol::CodexPermissionModeReport,
    ) -> ConversationRecordFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn record_failure<'a>(&'a self, _: &'a OperationId, _: bool) -> ConversationRecordFuture<'a> {
        Box::pin(async { Ok(()) })
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
            "v2":definitions,
            "ServerRequest":{"type":"object","required":["id","method","params"],"properties":{
                "id":{"type":["integer","string"]},"method":{"const":"item/commandExecution/requestApproval"},
                "params":{"type":"object","required":["threadId","turnId","itemId"],"properties":{
                    "threadId":{"type":"string"},"turnId":{"type":"string"},"itemId":{"type":"string"},"command":{"type":["string","null"]}
                }}
            }},
            "ServerNotification":{"type":"object","required":["method","params"],"properties":{
                "method":{"enum":["item/agentMessage/delta","item/started","item/completed","turn/completed"]},"params":{"type":"object"}
            }}
        }}))
        .unwrap_or_else(|error| panic!("schema JSON: {error}")),
    )]))
    .unwrap_or_else(|error| panic!("bundle: {error}"));
    Arc::new(
        NativePayloadSchemas::from_bundle(&bundle)
            .unwrap_or_else(|error| panic!("schemas: {error}")),
    )
}

/// A resumed idle binding whose Codex peer follows `script` afterwards, with the fake's live
/// transcript.
async fn binding_with_codex_script(
    catalog: &mut AcpSchemaCatalog,
    script: Vec<Step>,
) -> (
    AcpSessionBinding,
    tokio::task::JoinHandle<()>,
    CodexTranscript,
) {
    let (client, server) = tokio::net::UnixStream::pair().unwrap_or_else(|e| panic!("pair: {e}"));
    let connection = NativeProtocolConnection::from_websocket(
        WebSocketStream::from_raw_socket(client, Role::Client, None).await,
    );
    let mut full_script = vec![Step::Request(
        "thread/resume",
        json!({"cwd":"/work","thread":{"id":"thread-a","cwd":"/work","status":{"type":"idle"},"turns":[]}}),
    )];
    full_script.extend(script);
    let transcript = CodexTranscript::default();
    let fixture = spawn_fake_codex(
        WebSocketStream::from_raw_socket(server, Role::Server, None).await,
        full_script,
        transcript.clone(),
    );
    let generation: CodexGeneration = serde_json::from_value(
        json!({"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1}),
    )
    .unwrap_or_else(|error| panic!("generation: {error}"));
    let (binding, _history) = AcpSessionBinding::load_existing(
        catalog,
        SessionSetupInputs {
            scratch_scopes: Arc::new(IgnoredScratchScopes),
            operation_id: None,
            recorder: Arc::new(AcceptingRecorder),
            connection,
            schemas: native_schemas(),
            generation,
            params: json!({"sessionId":"thread-a","cwd":"/work","mcpServers":[],"_meta":{"codex-router/replayHistory":false}}),
        },
    )
    .await
    .unwrap_or_else(|error| panic!("resume binding: {error}"));
    (binding, fixture, transcript)
}

fn prompt_params() -> Value {
    json!({"sessionId":"thread-a","prompt":[{"type":"text","text":"hello"}]})
}

/// The two Codex calls that start a turn, as the fake Codex answers them.
fn start_turn_steps() -> Vec<Step> {
    vec![
        Step::Request(
            "thread/read",
            json!({"thread":{"id":"thread-a","status":{"type":"idle"},"turns":[]}}),
        ),
        Step::Request("turn/start", json!({"turn":{"id":"turn-a"}})),
    ]
}

fn state_of(prompt: &PendingAcpPrompt) -> NodeStateSnapshot {
    NodeStateSnapshot::new()
        .field("turn_id", prompt.settlement.turn_id().unwrap_or("none"))
        .field("blocks_next_prompt", prompt.blocks_next_prompt())
}

/// Records one event `next_event` returned. Each is a Rust API result, never a frame on the
/// carrier: the ACP updates and the terminal response are the values the connection would
/// write, and the native notifications and callbacks are handed to their owners.
fn record_prompt_event(case: &mut TapCase, event: &PromptEvent) {
    case.connection(ACP_CARRIER_CONNECTION);
    let family = MessageFamily::AcpServedForCodexSessions;
    let counterpart = EdgeParty::McpCaller;
    match event {
        PromptEvent::Update(value) => case.produced_with(
            RecordLayer::TypedOutcome,
            family,
            counterpart,
            &format!(
                "session/update:{}",
                value["params"]["update"]["sessionUpdate"]
                    .as_str()
                    .unwrap_or("unknown")
            ),
            value.clone(),
        ),
        // The terminal response is the "ACP served for Codex sessions" message N8 produces for its
        // client (the table's row for it), not a reply filed under the prompt's own row.
        PromptEvent::Terminal(value) => case.produced_with(
            RecordLayer::TypedOutcome,
            family,
            counterpart,
            &format!(
                "response:session/prompt stopReason={}",
                value["result"]["stopReason"].as_str().unwrap_or("none")
            ),
            value.clone(),
        ),
        PromptEvent::NativeNotification(value) => case.produced_with(
            RecordLayer::TypedOutcome,
            family,
            counterpart,
            &format!(
                "native-notification:{}",
                value["method"]
                    .as_str()
                    .or_else(|| value["kind"].as_str())
                    .unwrap_or("unknown")
            ),
            value.clone(),
        ),
        PromptEvent::InteractionRefused(_) => {
            panic!("unexpected refusal in a non-interaction case")
        }
        PromptEvent::NativeCallback(value) => case.produced_with(
            RecordLayer::TypedOutcome,
            family,
            counterpart,
            &format!("native-callback:{}", value["method"].as_str().unwrap_or("")),
            value.clone(),
        ),
    }
}

fn new_case(case: &str, row: &str) -> TapCase {
    TapCase::new(ProtocolNode::N8CodexAcpAdapter, case, row)
}

/// The prompt as the adapter is handed it: the `session/prompt` params, its id given apart.
fn consume_acp_prompt(case: &mut TapCase) {
    // Table gap: no family names ACP consumed by N8 from the carrier.
    case.connection(ACP_CARRIER_CONNECTION);
    case.consumed_raw(
        RecordLayer::Body {
            body: BodyRoot::AcpRequestParams {
                method: "session/prompt".into(),
            },
        },
        MessageFamily::AcpRequestToCodexAdapter,
        EdgeParty::McpCaller,
        "session/prompt",
        prompt_params(),
        &[],
    );
}

/// A request frame the adapter should send to Codex; the id is the adapter's own request counter.
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

/// Setup on the Codex socket before a case's own messages: the binding's `thread/resume`.
const RESUMED_SESSION_SETUP_REQUESTS: usize = 1;

/// Setup before a case that starts with a turn in flight: the resume, then the prompt's
/// `thread/read` and `turn/start`.
const STARTED_TURN_SETUP_REQUESTS: usize = 3;

/// A turn in flight blocks the next prompt until it settles.
fn turn_state(turn: &str, _permissions: u64, blocks_next_prompt: bool) -> NodeStateSnapshot {
    NodeStateSnapshot::new()
        .field("turn_id", turn)
        .field("blocks_next_prompt", blocks_next_prompt)
}

#[tokio::test]
async fn node_tap_acp_prompt_becomes_codex_thread_read_and_turn_start() {
    let mut catalog = AcpSchemaCatalog::load().unwrap_or_else(|e| panic!("catalog: {e}"));
    let (binding, fixture, transcript) =
        binding_with_codex_script(&mut catalog, start_turn_steps()).await;
    let mut case = new_case(
        "acp_prompt_to_codex_calls",
        "N8 section: ACP prompt from the carrier -> Codex calls to N10",
    );
    case.state_before(NodeStateSnapshot::new().field("turn_id", "none"));
    consume_acp_prompt(&mut case);
    let prompt = PendingAcpPrompt::start(binding, &mut catalog, json!(PROMPT_ID), &prompt_params())
        .await
        .unwrap_or_else(|error| panic!("start: {error}"));
    case.state_after(state_of(&prompt));
    drop(prompt);
    fixture.await.unwrap_or_else(|e| panic!("fixture: {e}"));
    CodexWireRecorder::with_setup_requests(RESUMED_SESSION_SETUP_REQUESTS)
        .record_new(&mut case, &transcript);
    let row = n10_call(
        n10_call(
            CatalogRowExpectation::new(),
            "thread/read",
            "agent-collaboration-1",
            json!({"threadId":"thread-a","includeTurns":false}),
        ),
        "turn/start",
        "agent-collaboration-2",
        json!({"threadId":"thread-a","input":[{"type":"text","text":"hello","text_elements":[]}]}),
    )
    .state_after(turn_state("turn-a", 0, true));
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_codex_agent_message_delta_becomes_acp_message_chunk() {
    let mut catalog = AcpSchemaCatalog::load().unwrap_or_else(|e| panic!("catalog: {e}"));
    let mut script = start_turn_steps();
    script.push(Step::Send(json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-a","turnId":"turn-a","itemId":"message-a","delta":"hi there"}})));
    let (binding, fixture, transcript) = binding_with_codex_script(&mut catalog, script).await;
    let mut prompt =
        PendingAcpPrompt::start(binding, &mut catalog, json!(PROMPT_ID), &prompt_params())
            .await
            .unwrap_or_else(|error| panic!("start: {error}"));
    let mut case = new_case(
        "codex_notification_to_acp_update",
        "N8 section: Codex notifications from N10 -> ACP updates to the carrier",
    );
    case.state_before(state_of(&prompt));
    // The setup frames and the scripted Codex message, as the fake saw them so far.
    let mut codex_wire = CodexWireRecorder::with_setup_requests(STARTED_TURN_SETUP_REQUESTS);
    codex_wire.record_new(&mut case, &transcript);
    let event = prompt
        .next_event(&mut catalog)
        .await
        .unwrap_or_else(|error| panic!("event: {error}"))
        .unwrap_or_else(|| panic!("an event"));
    record_prompt_event(&mut case, &event);
    case.state_after(state_of(&prompt));
    drop(prompt);
    fixture.await.unwrap_or_else(|e| panic!("fixture: {e}"));
    // Whatever else crossed the socket, the adapter's replies included.
    codex_wire.record_new(&mut case, &transcript);
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::AcpServedForCodexSessions,
            EdgeParty::McpCaller,
            "session/update:agent_message_chunk",
            json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"thread-a","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi there"}}}}),
        )
        .state_after(turn_state("turn-a", 0, true));
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_codex_command_item_becomes_acp_tool_call() {
    let mut catalog = AcpSchemaCatalog::load().unwrap_or_else(|e| panic!("catalog: {e}"));
    let mut script = start_turn_steps();
    let started = json!({"method":"item/started","params":{"threadId":"thread-a","turnId":"turn-a","startedAtMs":1,"item":{"type":"commandExecution","id":"tool-a","command":"cargo test","status":"inProgress","commandActions":[],"cwd":"/work"}}});
    script.push(Step::Send(started.clone()));
    let (binding, fixture, transcript) = binding_with_codex_script(&mut catalog, script).await;
    let mut prompt =
        PendingAcpPrompt::start(binding, &mut catalog, json!(PROMPT_ID), &prompt_params())
            .await
            .unwrap_or_else(|error| panic!("start: {error}"));
    let mut case = new_case(
        "codex_tool_item_to_acp_tool_call",
        "N8 section: Codex notifications from N10 -> ACP updates to the carrier",
    );
    case.state_before(state_of(&prompt));
    // The setup frames and the scripted Codex message, as the fake saw them so far.
    let mut codex_wire = CodexWireRecorder::with_setup_requests(STARTED_TURN_SETUP_REQUESTS);
    codex_wire.record_new(&mut case, &transcript);
    let event = prompt
        .next_event(&mut catalog)
        .await
        .unwrap_or_else(|error| panic!("event: {error}"))
        .unwrap_or_else(|| panic!("an event"));
    record_prompt_event(&mut case, &event);
    case.state_after(state_of(&prompt));
    drop(prompt);
    fixture.await.unwrap_or_else(|e| panic!("fixture: {e}"));
    // Whatever else crossed the socket, the adapter's replies included.
    codex_wire.record_new(&mut case, &transcript);
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::AcpServedForCodexSessions,
            EdgeParty::McpCaller,
            "session/update:tool_call",
            json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"thread-a","update":{"sessionUpdate":"tool_call","toolCallId":"tool-a","kind":"execute","title":"cargo test","status":"in_progress","content":[],"locations":[],"rawOutput":{"type":"commandExecution","id":"tool-a","command":"cargo test","status":"inProgress","commandActions":[],"cwd":"/work"}}}}),
        )
        .state_after(turn_state("turn-a", 0, true));
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_codex_turn_completed_becomes_acp_prompt_response() {
    let mut catalog = AcpSchemaCatalog::load().unwrap_or_else(|e| panic!("catalog: {e}"));
    let mut script = start_turn_steps();
    let completed = json!({"method":"turn/completed","params":{"threadId":"thread-a","turn":{"id":"turn-a","items":[],"status":"completed"}}});
    script.push(Step::Send(completed.clone()));
    script.push(Step::Request(
        "thread/read",
        json!({"thread":{"id":"thread-a","model":"gpt-5.6-sol","reasoningEffort":"medium","createdAt":1}}),
    ));
    let (binding, fixture, transcript) = binding_with_codex_script(&mut catalog, script).await;
    let mut prompt =
        PendingAcpPrompt::start(binding, &mut catalog, json!(PROMPT_ID), &prompt_params())
            .await
            .unwrap_or_else(|error| panic!("start: {error}"));
    let mut case = new_case(
        "codex_turn_completed_to_prompt_response",
        "N8 section: Codex notifications from N10 -> ACP updates (terminal response) to the carrier",
    );
    case.state_before(state_of(&prompt));
    // The setup frames and the scripted Codex message, as the fake saw them so far.
    let mut codex_wire = CodexWireRecorder::with_setup_requests(STARTED_TURN_SETUP_REQUESTS);
    codex_wire.record_new(&mut case, &transcript);
    let event = prompt
        .next_event(&mut catalog)
        .await
        .unwrap_or_else(|error| panic!("event: {error}"))
        .unwrap_or_else(|| panic!("an event"));
    case.state_after(state_of(&prompt));
    drop(prompt);
    fixture.await.unwrap_or_else(|e| panic!("fixture: {e}"));
    // Whatever else crossed the socket, the adapter's replies included.
    codex_wire.record_new(&mut case, &transcript);
    record_prompt_event(&mut case, &event);
    let row = n10_call(
        CatalogRowExpectation::new(), "thread/read", "agent-collaboration-3", json!({"threadId":"thread-a","includeTurns":false}))
        .produces_with(
            MessageFamily::AcpServedForCodexSessions,
            EdgeParty::McpCaller,
            "response:session/prompt stopReason=end_turn",
            json!({"jsonrpc":"2.0","id":"acp-prompt","result":{"stopReason":"end_turn","_meta":{"codex-router/finalReply":{"kind":"available","text":null},"codexRouter":{"effectiveModel":"gpt-5.6-sol","effectiveEffort":"medium","effectiveAccess":null,"settingsObservation":{"kind":"unavailable","reason":"noRecordedAccessRoute"},"idleSeconds":0}}}}),
        )
        .state_after(turn_state("turn-a", 0, false));
    case.finish(&row).assert_settled();
}
