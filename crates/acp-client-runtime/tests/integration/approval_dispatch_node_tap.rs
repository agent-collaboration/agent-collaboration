//! N6 to N7 tap: an agent permission request becomes an approval request for the
//! broker, and the broker's selected option becomes the ACP permission outcome.
//!
//! The agent is the scripted stdio fixture the other approval cases use; the
//! outcome record comes from the receipt the fixture writes when the agent
//! receives the client's answer.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::acp_owner_fixture::FixtureClient as AgentSessionClient;
use acp_client_runtime::{
    ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch, HistoryReplayFuture,
    InteractionFuture, InteractionPort, ProviderPersistenceTarget, RefusedApprovalOffer,
    SessionEventSink,
};
use codex_router_test_support::node_taps::{
    CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode, RecordLayer,
    TapCase,
};
use session_event_model::{ApprovalRequest, SessionEvent};
use tokio_util::sync::CancellationToken;

use crate::acp_wire_capture::{
    AgentSide, CapturedFrame, HELD_TURN_SETUP, frames_of, read_capture, record_frames,
};

const PERMISSION_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/permission-wire-agent.ts"
);
const BROKER: EdgeParty = EdgeParty::Node(ProtocolNode::N7ApprovalsBroker);
const AGENT: EdgeParty = EdgeParty::AcpAgent;

#[derive(Default)]
struct BrokerStandIn {
    accepted: Mutex<Vec<ApprovalRequest>>,
    refused: Mutex<Option<RefusedApprovalOffer>>,
    /// Every decision the stand-in handed back, whole.
    decisions: Mutex<Vec<ApprovalPortOutcome>>,
}

impl InteractionPort for BrokerStandIn {
    type Context = ();
    type OperationId = u64;

    fn operation_id(_context: &Self::Context) -> Self::OperationId {
        1
    }
    fn binding_retirement(_context: &Self::Context) -> CancellationToken {
        CancellationToken::new()
    }

    #[expect(
        clippy::expect_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    fn request_approval(
        &self,
        _context: Self::Context,
        request: ApprovalRequest,
        _turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        Box::pin(async move {
            self.accepted.lock().expect("capture lock").push(request);
            let decision = ApprovalPortOutcome::Selected {
                option_id: "allow-always".to_owned(),
                note: None,
            };
            self.decisions
                .lock()
                .expect("capture lock")
                .push(decision.clone());
            decision
        })
    }

    fn request_question(
        &self,
        _context: Self::Context,
        _request: session_event_model::QuestionRequest,
        _turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, session_event_model::QuestionResponse> {
        Box::pin(async { session_event_model::QuestionResponse::Cancelled })
    }

    #[expect(
        clippy::expect_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    fn record_refusal(
        &self,
        _context: Self::Context,
        refusal: RefusedApprovalOffer,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async move {
            *self.refused.lock().expect("capture lock") = Some(refusal);
        })
    }

    fn cancel_all(
        &self,
        _context: Self::Context,
        _reason: &'static str,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
}

struct DiscardingEventSink;

impl SessionEventSink for DiscardingEventSink {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        Ok(())
    }

    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn publish(&self, _session_id: &str, _event: SessionEvent) -> Result<(), EventSinkClosed> {
        Ok(())
    }
}

#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
async fn run_permission_fixture(mode: &str) -> (Arc<BrokerStandIn>, Vec<CapturedFrame>) {
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.jsonl");
    let port = Arc::new(BrokerStandIn::default());
    let client = AgentSessionClient::initialize(
        ExternalProviderLaunch {
            executable: PathBuf::from("node"),
            arguments: vec![
                PERMISSION_FIXTURE.to_owned(),
                mode.to_owned(),
                receipt.to_string_lossy().into_owned(),
            ],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::CursorAllowlist,
        },
        Arc::clone(&port),
        Arc::new(DiscardingEventSink),
        Arc::new(crate::test_behavior::TestProviderBehavior),
    )
    .await
    .expect("fixture initializes");
    client
        .create_session(root.path().to_path_buf())
        .await
        .expect("session opens");
    let result = client
        .submit_with_approval_dispatch_for_input(
            "fixture-session".to_owned(),
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("Run it".to_owned())
                        .expect("text prompt"),
                ]),
            ),
            (),
            None,
        )
        .await;
    client.shutdown().await;
    assert!(result.is_ok(), "prompt result: {result:?}");
    (port, read_capture(&receipt))
}

/// Renders the outcome in the whole response the agent received.
fn permission_outcome_kind(answer: &serde_json::Value) -> String {
    let field = |pointer: &str| answer.pointer(pointer).and_then(serde_json::Value::as_str);
    match field("/result/outcome/outcome") {
        Some("selected") => format!(
            "session/request_permission:selected({})",
            field("/result/outcome/optionId").unwrap_or("?")
        ),
        Some(other) => format!("session/request_permission:{other}"),
        None => "session/request_permission:malformed".to_owned(),
    }
}

#[tokio::test]
async fn duplicate_callback_in_a_batch_gets_one_reply_and_keeps_its_valid_sibling() {
    let (port, capture) = run_permission_fixture("duplicate-batch").await;
    let received: Vec<_> = capture
        .iter()
        .filter(|frame| frame.agent_side == AgentSide::Received)
        .map(|captured| &captured.frame)
        .collect();
    let batch = received
        .iter()
        .find_map(|frame| frame.as_array())
        .expect("provider received a batch reply");
    let batch_ids: Vec<_> = batch.iter().map(|reply| &reply["id"]).collect();
    assert_eq!(
        batch_ids,
        [&serde_json::json!(91), &serde_json::json!(92)],
        "the duplicate gets no batch response slot: {batch:?}"
    );
    for reply in batch {
        assert_eq!(
            permission_outcome_kind(reply),
            "session/request_permission:selected(allow-always)"
        );
        assert!(reply.get("error").is_none(), "no internal error: {reply}");
    }
    let reused: Vec<_> = received.iter().filter(|reply| reply["id"] == 91).collect();
    assert_eq!(
        reused.len(),
        1,
        "a flushed ID may be reused on the next request"
    );
    assert_eq!(
        permission_outcome_kind(reused[0]),
        "session/request_permission:selected(allow-always)"
    );
    assert_eq!(port.accepted.lock().expect("broker calls").len(), 3);
}

/// Records what the permission request caused inside the Router, at its place in the capture:
/// the approval request N6 handed the broker and the decision the broker handed back.
#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
fn record_broker_exchange(case: &mut TapCase, port: &BrokerStandIn) {
    let requests = port.accepted.lock().expect("capture lock").clone();
    assert!(!requests.is_empty(), "request reached the broker stand-in");
    for request in &requests {
        let option_ids = request
            .options
            .iter()
            .map(|option| option.option_id.as_str().to_owned())
            .collect::<Vec<_>>()
            .join(",");
        case.produced_with(
            RecordLayer::TypedOutcome,
            MessageFamily::ApprovalRequest,
            BROKER,
            &format!("ApprovalRequest(options={option_ids})"),
            serde_json::to_value(request).expect("request serializes"),
        );
    }
    // The whole decision the broker handed back (it has no serde, so its full Debug text).
    let decisions = port.decisions.lock().expect("capture lock").clone();
    assert_eq!(decisions.len(), 1, "one decision per request");
    let decision = decisions.first().expect("the one decision");
    case.consumed_with(
        RecordLayer::TypedInput,
        MessageFamily::ApprovalDecision,
        BROKER,
        "selected option allow-always",
        serde_json::json!({ "debug": format!("{decision:?}") }),
    );
}

/// Catalog: Program Design S3 "Approvals, both ways" and the N7 section: the
/// request goes to the broker as a vocabulary approval request, the selected
/// option comes back and answers the agent as an ACP permission outcome.
#[tokio::test]
async fn node_tap_permission_request_and_selected_option_round_trip() {
    let mut case = TapCase::new(
        ProtocolNode::N6AcpClient,
        "permission_request_selected_option",
        "N7 section and S3: N6 permission request -> N7 broker; N7 selected option -> N6 -> ACP outcome",
    );
    case.state_before(NodeStateSnapshot::new().field("pending_requests_at_broker", "none"));
    let (port, capture) = run_permission_fixture("persistent").await;
    // The whole JSON-RPC request the agent sent, and the whole response it received.
    assert_eq!(
        frames_of(&capture, AgentSide::Sent, "session/request_permission"),
        vec![&serde_json::json!({
            "jsonrpc": "2.0", "id": 91, "method": "session/request_permission",
            "params": {
                "sessionId": "fixture-session",
                "toolCall": {
                    "toolCallId": "tool-1", "title": "Run command", "kind": "execute",
                    "rawInput": { "command": "echo safe", "api_key": "secret" }
                },
                "options": [{ "optionId": "allow-always", "name": "Always allow", "kind": "allow_always" }]
            }
        })],
        "the whole request the agent put on the wire"
    );
    // The outcome is the reply to the agent's own request, on its row (agent to N6).
    // The session and the prompt that opens the turn are fixture setup; the row is the approval.
    record_frames(
        &mut case,
        None,
        &capture,
        HELD_TURN_SETUP,
        &|method, reply| {
            (method == "session/request_permission").then(|| permission_outcome_kind(reply))
        },
        &mut |case, captured| {
            if captured.agent_side == AgentSide::Sent
                && captured.method() == Some("session/request_permission")
            {
                record_broker_exchange(case, &port);
            }
        },
    );
    case.state_after(NodeStateSnapshot::new().field("pending_requests_at_broker", "none"));
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::ApprovalRequest,
            BROKER,
            "ApprovalRequest(options=allow-always)",
            serde_json::json!({
                "requestId": "[\"fixture-session\",91]",
                "title": "Run command",
                "description": "command: echo safe",
                "subject": { "type": "tool_call", "toolCall": {
                    "toolCallId": "tool-1", "kind": "Execute", "title": "Run command" } },
                "optionsOrigin": "agentOffered",
                "options": [{
                    "optionId": "allow-always",
                    "label": "Always allow",
                    "choice": { "effect": "allow", "scope": { "persistent": {
                        "where_stored": "Cursor allowlist (Cursor decides whether global or per-project)" } } }
                }]
            }),
        )
        .produces_with(
            MessageFamily::AcpSessionUpdate,
            AGENT,
            "session/request_permission:selected(allow-always)",
            serde_json::json!({
                "jsonrpc": "2.0", "id": 91,
                "result": { "outcome": { "outcome": "selected", "optionId": "allow-always" } }
            }),
        )
        .state_after(NodeStateSnapshot::new().field("pending_requests_at_broker", "none"));
    case.finish(&row).assert_settled();
}
