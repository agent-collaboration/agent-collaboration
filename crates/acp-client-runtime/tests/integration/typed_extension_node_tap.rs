//! N6 tap cases for the typed extension messages: `_session/steering` and
//! `cursor/list_available_models`.
//!
//! The agent is `tests/support/typed-extension-agent.ts`. Its shared transport
//! writes the complete wire capture (every line both ways, from `initialize`
//! on), and each case records all of it as raw wire frames before it looks at
//! its own messages. The other records come from the recording event sink and
//! the call's result; the catalog rows are written out in each case's
//! expectation.

#![expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]

mod fixture_rig;

use acp_client_runtime::{
    ExternalProviderRuntimeError, ProviderSessionActivity, ProviderSteeringOutcome,
};
use codex_router_test_support::node_taps::{
    AcpPeer, CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode,
    RecordIntent, RecordLayer, TapCase,
};
use provider_session_machines::{
    ModelReasoning, ModelReasoningEntry, ModelReasoningMetadata, ModelReasoningUnknown,
};
use session_event_model::{InputId, SessionEvent};

use crate::acp_wire_capture::{
    AgentSide, CapturedFrame, HELD_TURN_SETUP, SESSION_SETUP, frames_of, record_frames, replies_to,
};
use fixture_rig::{Wire, activity_text, drain_events, event_records, submission};

const AGENT: EdgeParty = EdgeParty::AcpAgent;
const HUB: EdgeParty = EdgeParty::Node(ProtocolNode::N5EventHub);
const COMMAND_PORT: EdgeParty = EdgeParty::Node(ProtocolNode::N4CommandPort);
const SESSION_ID: &str = "fixture-session";

/// Placeholder for the JSON-RPC request id: the ACP SDK mints a fresh UUID for every request.
const VOLATILE_REQUEST_ID: &str = "<volatile:id>";

/// The whole replies the agent sent to N6's `method` requests, with only the top-level JSON-RPC
/// id masked (nested ids, such as a config option "id", are content). This is the copy the test
/// asserts on; the recorded wire frames keep the exact id.
fn masked_replies(capture: &[CapturedFrame], method: &str) -> Vec<serde_json::Value> {
    replies_to(capture, AgentSide::Sent, method)
        .into_iter()
        .map(|reply| {
            let mut reply = reply.clone();
            reply
                .as_object_mut()
                .expect("JSON-RPC message object")
                .insert("id".to_owned(), serde_json::json!(VOLATILE_REQUEST_ID));
            reply
        })
        .collect()
}

fn steering_result_kind(
    result: &Result<ProviderSteeringOutcome<u64, ()>, ExternalProviderRuntimeError>,
) -> &'static str {
    match result {
        Ok(ProviderSteeringOutcome::Injected { .. }) => "steering result:Injected",
        Ok(ProviderSteeringOutcome::StartedNewTurn) => "steering result:StartedNewTurn",
        Ok(ProviderSteeringOutcome::PromptRequired) => "steering result:PromptRequired",
        Ok(ProviderSteeringOutcome::Failed) => "steering result:Failed",
        Err(ExternalProviderRuntimeError::Operation(text))
            if text.contains("invalid steering result") =>
        {
            "steering error:invalid reply"
        }
        Err(_) => "steering error:other",
    }
}

/// The whole reply the agent fixture sent to the steering request, written from literals: the result
/// it scripts (the id is the SDK-minted request id).
fn assert_steering_reply(run: &SteerRun, result: serde_json::Value) {
    assert_eq!(
        masked_replies(&run.capture, "_session/steering"),
        vec![serde_json::json!({
            "jsonrpc": "2.0", "id": VOLATILE_REQUEST_ID, "result": result
        })],
        "the whole reply the agent sent on the wire"
    );
}

struct SteerRun {
    /// The fixture's complete wire capture, from `initialize` to shutdown.
    capture: Vec<CapturedFrame>,
    /// The capture's fixture setup: the session, and a held turn when there is one.
    setup_methods: &'static [&'static str],
    events: Vec<SessionEvent>,
    result: Result<ProviderSteeringOutcome<u64, ()>, ExternalProviderRuntimeError>,
    activity_before: ProviderSessionActivity,
    activity_after: ProviderSessionActivity,
}

/// Idle steer: the session has no turn when the agent is asked.
async fn steer_while_idle(scenario: &str) -> SteerRun {
    let mut wire = Wire::start(scenario).await;
    wire.open_session().await;
    let activity_before = wire
        .client
        .session_activity(SESSION_ID.to_owned())
        .expect("activity");
    let result = wire
        .client
        .steer_contents_with_input(
            SESSION_ID.to_owned(),
            InputId::new("tap-steer").expect("input id"),
            submission("steer text"),
        )
        .await;
    let events = drain_events(&mut wire.events);
    let activity_after = wire
        .client
        .session_activity(SESSION_ID.to_owned())
        .expect("activity");
    wire.client.shutdown().await;
    SteerRun {
        capture: wire.capture(),
        setup_methods: SESSION_SETUP,
        events,
        result,
        activity_before,
        activity_after,
    }
}

/// Running steer: a first prompt holds a turn open until the agent answers.
async fn steer_while_running(scenario: &str) -> SteerRun {
    let mut wire = Wire::start(scenario).await;
    wire.open_session().await;
    let prompt = wire.client.submit_with_approval_dispatch_for_input(
        SESSION_ID.to_owned(),
        InputId::new("tap-prompt").expect("input id"),
        acp_client_runtime::ProviderTurnInput::Prompt(submission("First")),
        (),
        None,
    );
    tokio::pin!(prompt);
    let started = tokio::select! {
        event = wire.events.recv() => event.expect("turn start event"),
        result = &mut prompt => panic!("prompt settled before turn began: {result:?}"),
    };
    assert!(matches!(started, SessionEvent::TurnStarted { .. }));
    // The prompt's own user message item.
    wire.events.recv().await.expect("prompt item started");
    wire.events.recv().await.expect("prompt item completed");
    let activity_before = wire
        .client
        .session_activity(SESSION_ID.to_owned())
        .expect("activity");
    let result = wire
        .client
        .steer_contents_with_input(
            SESSION_ID.to_owned(),
            InputId::new("tap-steer").expect("input id"),
            submission("steer text"),
        )
        .await;
    let events = drain_events(&mut wire.events);
    let activity_after = wire
        .client
        .session_activity(SESSION_ID.to_owned())
        .expect("activity");
    if scenario != "steer-prompt-required" {
        wire.client
            .cancel_active_prompt(SESSION_ID.to_owned())
            .await
            .expect("cancel the held turn");
    }
    prompt.await.expect("prompt ends");
    wire.client.shutdown().await;
    SteerRun {
        capture: wire.capture(),
        setup_methods: HELD_TURN_SETUP,
        events,
        result,
        activity_before,
        activity_after,
    }
}

const STEERING_ROW: &str = "N6 catalog: Typed extension messages: Steering";

/// The steering call's whole result as its full `Debug` text (the outcome type has no serde).
fn result_value(
    result: &Result<ProviderSteeringOutcome<u64, ()>, ExternalProviderRuntimeError>,
) -> serde_json::Value {
    serde_json::json!({ "debug": format!("{result:?}") })
}

/// Whether `captured` is the agent's reply to N6's `_session/steering` request.
fn is_steering_reply(captured: &CapturedFrame, capture: &[CapturedFrame]) -> bool {
    replies_to(capture, AgentSide::Sent, "_session/steering").contains(&&captured.frame)
}

/// Records a steer case. `reply_intent` says what the fixture's scripted steering reply is when it
/// is not a faithful Claude reply (Claude extras pin, `SteerResponse`).
fn record_steer_case(
    case_id: &str,
    row_label: &str,
    run: &SteerRun,
    reply_intent: Option<RecordIntent>,
) -> TapCase {
    let mut case = TapCase::new(ProtocolNode::N6AcpClient, case_id, row_label);
    case.state_before(
        NodeStateSnapshot::new().field("session_activity", activity_text(run.activity_before)),
    );
    case.consumed(
        RecordLayer::TypedInput,
        MessageFamily::PromptContents,
        COMMAND_PORT,
        "SteerContents",
    );
    // The whole connection as it crossed; what the steering reply caused follows the reply.
    // The steering fixture stands in for Claude, whose upstream extension _session/steering is.
    record_frames(
        &mut case,
        Some(AcpPeer::Claude),
        &run.capture,
        run.setup_methods,
        &|method, _| (method == "_session/steering").then(|| "_session/steering reply".to_owned()),
        &mut |case, captured| {
            if !is_steering_reply(captured, &run.capture) {
                return;
            }
            if let Some(intent) = reply_intent.clone() {
                case.mark_last(intent);
            }
            for (kind, payload) in event_records(&run.events) {
                case.produced_with(
                    RecordLayer::TypedOutcome,
                    MessageFamily::SessionEvent,
                    HUB,
                    &kind,
                    payload,
                );
            }
            case.produced_reply_with(
                RecordLayer::TypedOutcome,
                MessageFamily::SessionCommand,
                COMMAND_PORT,
                steering_result_kind(&run.result),
                result_value(&run.result),
            );
        },
    );
    case.state_after(
        NodeStateSnapshot::new().field("session_activity", activity_text(run.activity_after)),
    );
    case
}

/// The whole steering request the node must put on the wire, written from literals: the
/// session, prompt text and idle behavior it was fed. The JSON-RPC `id` is a fresh UUID the ACP
/// SDK mints per request, so it is the one placeholder (`<volatile:id>`).
fn steering_request() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": VOLATILE_REQUEST_ID,
        "method": "_session/steering",
        "params": {
            "sessionId": "fixture-session",
            "prompt": [{ "type": "text", "text": "steer text" }],
            "_meta": { "steering": { "idleBehavior": "promptRequired" } }
        }
    })
}

/// One expected session event: kind label and whole event, written from literals. Generated
/// turn and item ids are the only placeholders (a fresh UUIDv7 on every run).
type ExpectedEvent = (&'static str, serde_json::Value);

fn input_accepted() -> ExpectedEvent {
    (
        "InputAccepted",
        serde_json::json!({ "kind": "inputAccepted", "inputId": "tap-steer",
            "turnId": "<volatile:turnId>" }),
    )
}

fn user_message_started() -> ExpectedEvent {
    (
        "ItemStarted(UserMessage)",
        serde_json::json!({ "kind": "itemStarted", "item": {
            "itemId": "<volatile:itemId>", "kind": { "kind": "userMessage" }, "text": "steer text" } }),
    )
}

fn user_message_completed() -> ExpectedEvent {
    (
        "ItemCompleted(UserMessage)",
        serde_json::json!({ "kind": "itemCompleted", "itemId": "<volatile:itemId>" }),
    )
}

type SteeringResult = Result<ProviderSteeringOutcome<u64, ()>, ExternalProviderRuntimeError>;

/// The steer's own catalog row: the steering request, then the session events and the call's
/// result the steering reply caused. The session setup (and a held turn's prompt and cancel) are
/// recorded and checked as fixture setup, outside the row.
fn steer_expectation(
    events: Vec<ExpectedEvent>,
    result_kind: &str,
    result: &SteeringResult,
    activity: &str,
) -> CatalogRowExpectation {
    let mut row = CatalogRowExpectation::new().produces_with(
        MessageFamily::AcpToAgent,
        AGENT,
        "_session/steering",
        steering_request(),
    );
    for (kind, payload) in events {
        row = row.produces_with(MessageFamily::SessionEvent, HUB, kind, payload);
    }
    row.produces_with(
        MessageFamily::SessionCommand,
        COMMAND_PORT,
        result_kind,
        result_value(result),
    )
    .state_after(NodeStateSnapshot::new().field("session_activity", activity))
}

/// Catalog: injected while running publishes InputAccepted plus the user message.
#[tokio::test]
async fn node_tap_steering_injected_while_running() {
    let run = steer_while_running("steer-injected").await;
    assert_steering_reply(&run, serde_json::json!({ "outcome": "injected" }));
    let case = record_steer_case(
        "steering_injected",
        &format!("{STEERING_ROW}, injected while running"),
        &run,
        None,
    );
    let row = steer_expectation(
        vec![
            input_accepted(),
            user_message_started(),
            user_message_completed(),
        ],
        "steering result:Injected",
        &Ok(ProviderSteeringOutcome::Injected {
            running_operation_id: Some(1),
            // The running turn's approval context, captured with the steer
            // (design note §12.4).
            joined_turn: Some(()),
        }),
        "Running",
    );
    case.finish(&row).assert_settled();
}

/// An explicit steer while idle uses the normal prompt command and the same input ID.
#[tokio::test]
async fn node_tap_idle_steering_uses_one_normal_prompt() {
    let run = steer_while_idle("steer-idle-prompt").await;
    assert!(matches!(
        run.result,
        Ok(ProviderSteeringOutcome::StartedNewTurn)
    ));
    assert!(frames_of(&run.capture, AgentSide::Received, "_session/steering").is_empty());
    let prompts = frames_of(&run.capture, AgentSide::Received, "session/prompt");
    assert_eq!(prompts.len(), 1);
    assert_eq!(
        prompts[0]["params"],
        serde_json::json!({ "sessionId":SESSION_ID, "prompt":[{"type":"text", "text":"steer text"}] })
    );
    assert!(run.events.iter().any(|event| matches!(event, SessionEvent::TurnStarted { input_id, .. } if input_id.as_str() == "tap-steer")));
}

/// The definite promptRequired extension reply leads to one ordinary prompt.
#[tokio::test]
async fn node_tap_steering_prompt_required_routes_one_prompt() {
    let run = steer_while_running("steer-prompt-required").await;
    assert_steering_reply(
        &run,
        serde_json::json!({ "outcome": "promptRequired", "reason": "noRunningTurn" }),
    );
    let steers = frames_of(&run.capture, AgentSide::Received, "_session/steering");
    let prompts = frames_of(&run.capture, AgentSide::Received, "session/prompt");
    assert_eq!(steers.len(), 1);
    assert_eq!(prompts.len(), 2);
    assert_eq!(
        prompts[1]["params"]["prompt"],
        serde_json::json!([{"type":"text", "text":"steer text"}])
    );
    assert!(matches!(
        run.result,
        Ok(ProviderSteeringOutcome::StartedNewTurn)
    ));
    assert!(run.events.iter().any(|event| matches!(event, SessionEvent::TurnStarted { input_id, .. } if input_id.as_str() == "tap-steer")));
    assert!(!run.events.iter().any(|event| matches!(event, SessionEvent::InputAccepted { input_id, .. } if input_id.as_str() == "tap-steer")));
}

/// Catalog: the outcome is a closed enum; failed publishes nothing.
#[tokio::test]
async fn node_tap_steering_failed_publishes_nothing() {
    let run = steer_while_running("steer-failed").await;
    assert_steering_reply(&run, serde_json::json!({ "outcome": "failed" }));
    // Claude's steering contract has no `failed` outcome; N6 accepts one, so the fixture stands in
    // for a peer that sends it. The reply is apparatus: it does not represent Claude.
    let case = record_steer_case(
        "steering_failed",
        &format!("{STEERING_ROW}, failed"),
        &run,
        Some(RecordIntent::SyntheticApparatus),
    );
    let row = steer_expectation(
        vec![],
        "steering result:Failed",
        &Ok(ProviderSteeringOutcome::Failed),
        "Running",
    );
    case.finish(&row).assert_settled();
}

/// Catalog: any outcome outside the closed enum is an invalid-reply error.
#[tokio::test]
async fn node_tap_steering_outcome_outside_enum_is_invalid_reply() {
    let run = steer_while_running("steer-invalid").await;
    assert_steering_reply(&run, serde_json::json!({ "outcome": "sideways" }));
    let case = record_steer_case(
        "steering_invalid_outcome",
        &format!("{STEERING_ROW}, outcome outside the closed enum is an invalid-reply error"),
        &run,
        // The fixture injects an outcome outside the contract to prove N6 rejects it.
        Some(RecordIntent::DeliberateInvalid {
            keyword: "oneOf",
            instance_path: "/result",
            detail: "",
        }),
    );
    let row = steer_expectation(
        vec![],
        "steering error:invalid reply",
        &Err(ExternalProviderRuntimeError::Operation(
            "provider returned an invalid steering result".to_owned(),
        )),
        "Running",
    );
    case.finish(&row).assert_settled();
}

/// A model entry as the stand-in decoder builds it: the id from the reply, reasoning unknown.
fn model_entry(model_id: &str) -> ModelReasoningEntry {
    ModelReasoningEntry::try_new(
        model_id.to_owned(),
        ModelReasoning::Unknown(ModelReasoningUnknown::UnsupportedSource),
    )
    .expect("model id")
}

/// The whole decoded model metadata as its full `Debug` text.
fn model_reasoning_text(metadata: &ModelReasoningMetadata) -> String {
    format!("{metadata:?}")
}

/// The fixture's complete wire capture, the decoded model metadata, and the reply result the node
/// handed to the decoder (none when the agent answered with an error).
struct DiscoveryRun {
    capture: Vec<CapturedFrame>,
    metadata: ModelReasoningMetadata,
    reply_seen: Option<serde_json::Value>,
}

async fn run_discovery(scenario: &str) -> DiscoveryRun {
    let mut wire = Wire::start(scenario).await;
    wire.open_session().await;
    let catalog = wire
        .client
        .settings_catalog(SESSION_ID)
        .expect("settings catalog");
    let reply_seen = wire.behavior.reply_seen.lock().expect("reply lock").clone();
    wire.client.shutdown().await;
    DiscoveryRun {
        capture: wire.capture(),
        metadata: catalog.model_reasoning,
        reply_seen,
    }
}

fn discovery_case(case_id: &str, row_label: &str, run: &DiscoveryRun, reply_kind: &str) -> TapCase {
    let mut case = TapCase::new(ProtocolNode::N6AcpClient, case_id, row_label);
    case.state_before(NodeStateSnapshot::new().field("connection_models", "not yet read"));
    // The discovery fixture stands in for Cursor, whose extension cursor/list_available_models is.
    record_frames(
        &mut case,
        Some(AcpPeer::Cursor),
        &run.capture,
        SESSION_SETUP,
        &|method, _| (method == "cursor/list_available_models").then(|| reply_kind.to_owned()),
        &mut |_, _| {},
    );
    case.state_after(
        NodeStateSnapshot::new().field("connection_models", model_reasoning_text(&run.metadata)),
    );
    case
}

/// The discovery's own catalog row: the discovery request. The session setup is recorded and
/// checked as fixture setup, outside the row.
fn discovery_expectation(metadata: &ModelReasoningMetadata) -> CatalogRowExpectation {
    CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::AcpToAgent,
            AGENT,
            "cursor/list_available_models",
            discovery_request(),
        )
        .state_after(
            NodeStateSnapshot::new().field("connection_models", model_reasoning_text(metadata)),
        )
}

/// The whole discovery request the node must put on the wire; its `id` is the SDK-minted UUID.
fn discovery_request() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": VOLATILE_REQUEST_ID,
        "method": "cursor/list_available_models",
        "params": {}
    })
}

/// Catalog: `cursor/list_available_models {}` answers with models; unknown
/// fields are allowed, so both model ids reach the catalog.
#[tokio::test]
async fn node_tap_cursor_model_discovery_success() {
    let run = run_discovery("models-ok").await;
    // The reply result the agent fixture sends, written out independently.
    let reply_result = serde_json::json!({
        "models": [
            { "value": "model-a", "name": "Model A", "unknownModelField": true,
              "configOptions": [{ "id": "effort", "name": "Effort", "category": "thought_level",
                "type": "select", "currentValue": "low",
                "options": [{ "value": "low", "name": "Low" }, { "value": "high", "name": "High" }] }] },
            { "value": "model-b", "name": "Model B", "configOptions": [] }
        ],
        "unknownTopLevelField": "ignored"
    });
    assert_eq!(
        run.reply_seen,
        Some(reply_result.clone()),
        "the reply the node handed to the decoder"
    );
    assert_eq!(
        masked_replies(&run.capture, "cursor/list_available_models"),
        vec![
            serde_json::json!({ "jsonrpc": "2.0", "id": VOLATILE_REQUEST_ID, "result": reply_result })
        ],
        "the whole reply the agent sent on the wire"
    );
    let case = discovery_case(
        "cursor_model_discovery_success",
        "N6 catalog: Typed extension messages: Cursor model discovery, models reply with unknown fields",
        &run,
        "models:[model-a,model-b]",
    );
    let row = discovery_expectation(&ModelReasoningMetadata::Available {
        per_model_descriptors: vec![model_entry("model-a"), model_entry("model-b")],
    });
    case.finish(&row).assert_settled();
}

/// Catalog: any failure means no model data is available.
#[tokio::test]
async fn node_tap_cursor_model_discovery_failure_means_no_model_data() {
    let run = run_discovery("models-error").await;
    assert_eq!(
        run.reply_seen, None,
        "an error reply has no result to decode"
    );
    assert_eq!(
        masked_replies(&run.capture, "cursor/list_available_models"),
        vec![serde_json::json!({
            "jsonrpc": "2.0", "id": VOLATILE_REQUEST_ID,
            "error": { "code": -32601, "message": "scripted discovery failure" }
        })],
        "the whole error reply the agent sent on the wire"
    );
    let case = discovery_case(
        "cursor_model_discovery_failure",
        "N6 catalog: Typed extension messages: Cursor model discovery, any failure means no model data",
        &run,
        "error:-32601",
    );
    let row = discovery_expectation(&ModelReasoningMetadata::Unknown(
        ModelReasoningUnknown::Unavailable,
    ));
    case.finish(&row).assert_settled();
}
