//! N3 node taps for the interaction rows of the conversion catalog:
//! InteractionRequested (1 server request, or 3 read-only items) and
//! InteractionResolved (0..n `serverRequest/resolved`).
use super::node_tap_tests::{
    Rig, base_state, expectation, frame, item_completed_frame, item_started_frame, tap_actor,
    tap_event, tap_event_normalized, text_delta_frame,
};
use super::*;
use crate::{NativeControlBackend, NativeGenerationGate};
use session_event_model::{
    ApprovalChoice, ApprovalEffect, ApprovalScope, ApprovalSubject, ApprovalToolCallSubject,
    OfferedOption, OfferedOptionId, OfferedOptions, OptionsOrigin,
};

const ROW_REQUESTED: &str = "N3 catalog: InteractionRequested";
const ROW_RESOLVED: &str = "N3 catalog: InteractionResolved";
fn read_only_frames(item_id: &str, text: &str) -> Vec<Value> {
    let item = json!({"type":"agentMessage","id":item_id,"text":text});
    vec![
        item_started_frame("", item.clone()),
        text_delta_frame("item/agentMessage/delta", "", item_id, text),
        item_completed_frame("", item),
    ]
}

/// A server request the CLI must answer: the id is deterministic, the start time is a clock read.
fn server_request(id: &str, method: &str, params: Value) -> Value {
    let mut expected = frame(method, params);
    expected["id"] = json!(id);
    expected
}

fn option(option_id: &str, effect: ApprovalEffect, scope: ApprovalScope) -> OfferedOption {
    OfferedOption {
        option_id: OfferedOptionId::new(option_id).expect("option id"),
        label: option_id.into(),
        choice: ApprovalChoice::new(effect, scope),
    }
}

fn approval(
    request_id: &str,
    subject: ApprovalSubject,
    options: Vec<OfferedOption>,
) -> ApprovalRequest {
    ApprovalRequest {
        request_id: request_id.into(),
        title: "Run it".into(),
        description: None,
        subject: Some(subject),
        options_origin: OptionsOrigin::AgentOffered,
        options: OfferedOptions::new(options).expect("options"),
    }
}

fn command_approval(request_id: &str) -> ApprovalRequest {
    approval(
        request_id,
        ApprovalSubject::Command {
            command: "pwd".into(),
            cwd: "/work".into(),
        },
        vec![
            option("allow", ApprovalEffect::Allow, ApprovalScope::Once),
            option("decline", ApprovalEffect::Decline, ApprovalScope::Once),
        ],
    )
}

fn question(request_id: &str) -> QuestionRequest {
    serde_json::from_value(json!({
        "requestId":request_id,"prompt":"Which one?","fields":[
            {"kind":"text","fieldId":"answer","label":"Answer","description":null,"required":true}
        ]
    }))
    .expect("question")
}

fn other_approver() -> Identity {
    serde_json::from_value(json!({"kind":"human","humanId":"someone-else"})).expect("identity")
}

async fn tap_broker(directory: &std::path::Path) -> Arc<ServiceInteractionBroker> {
    let session = super::node_tap_tests::tap_session();
    let store = Arc::new(
        interaction_storage::InteractionStore::open(directory.join("interaction.sqlite"))
            .await
            .expect("interaction store"),
    );
    ServiceInteractionBroker::load(
        session
            .endpoint
            .service_id
            .as_str()
            .to_owned()
            .try_into()
            .expect("service id"),
        NativeControlBackend {
            endpoint: serde_json::from_value(json!({
                "serviceId":session.endpoint.service_id,
                "endpointId":session.endpoint.endpoint_id
            }))
            .expect("endpoint"),
            gate: NativeGenerationGate::default(),
            codex_home: directory.join("codex-home"),
        },
        store,
    )
    .await
    .expect("broker")
}

fn requested(interaction: PendingInteraction) -> SessionEvent {
    SessionEvent::InteractionRequested { interaction }
}

fn interactive_case(
    case_name: &str,
    broker: Arc<ServiceInteractionBroker>,
    interaction: PendingInteraction,
    expected_request: Value,
    pending: &str,
) -> Vec<Value> {
    let mut rig = Rig::with_broker(Some(broker));
    let row = expectation(
        &[expected_request],
        base_state().field("pending_interactions", pending),
    );
    tap_event(
        &mut rig,
        case_name,
        ROW_REQUESTED,
        requested(interaction),
        &row,
    )
}

#[tokio::test]
async fn node_tap_interaction_requested_command_approval_is_one_server_request() {
    let directory = tempfile::tempdir().expect("tempdir");
    let broker = tap_broker(directory.path()).await;
    let frames = interactive_case(
        "interaction_requested_command_approval",
        broker,
        PendingInteraction::Approval {
            approver: tap_actor(),
            request: Box::new(command_approval("req-1")),
        },
        server_request(
            "router:approval:tap-session:req-1",
            "item/commandExecution/requestApproval",
            json!({
                "turnId":"","itemId":"req-1","startedAtMs":"<volatile:startedAtMs>",
                "reason":"Run it","availableDecisions":["accept","decline","cancel"],
                "command":"pwd","cwd":"/work"
            }),
        ),
        "approval:req-1",
    );
    assert_eq!(frames.len(), 1);
}

#[tokio::test]
async fn node_tap_interaction_requested_file_change_approval_is_one_server_request() {
    let directory = tempfile::tempdir().expect("tempdir");
    let broker = tap_broker(directory.path()).await;
    let request = approval(
        "req-2",
        ApprovalSubject::ToolCall {
            tool_call: ApprovalToolCallSubject {
                tool_call_id: "call-1".into(),
                kind: "edit".into(),
                title: "Edit".into(),
            },
        },
        vec![
            option("allow", ApprovalEffect::Allow, ApprovalScope::Once),
            option(
                "allow-session",
                ApprovalEffect::Allow,
                ApprovalScope::Session,
            ),
        ],
    );
    interactive_case(
        "interaction_requested_file_change_approval",
        broker,
        PendingInteraction::Approval {
            approver: tap_actor(),
            request: Box::new(request),
        },
        server_request(
            "router:approval:tap-session:req-2",
            "item/fileChange/requestApproval",
            json!({
                "turnId":"","itemId":"call-1","startedAtMs":"<volatile:startedAtMs>",
                "reason":"Run it"
            }),
        ),
        "approval:req-2",
    );
}

#[tokio::test]
async fn node_tap_interaction_requested_question_is_one_elicitation_form() {
    let directory = tempfile::tempdir().expect("tempdir");
    let broker = tap_broker(directory.path()).await;
    interactive_case(
        "interaction_requested_question_form",
        broker,
        PendingInteraction::Question {
            approver: tap_actor(),
            request: Box::new(question("q-1")),
        },
        server_request(
            "router:question:tap-session:q-1",
            "mcpServer/elicitation/request",
            json!({
                "turnId":"","serverName":"codex-router","mode":"form","message":"Which one?",
                "requestedSchema":{"type":"object","properties":{
                    "answer":{"type":"string","title":"Answer","description":null}
                },"required":["answer"]}
            }),
        ),
        "question:q-1",
    );
}

#[tokio::test]
async fn node_tap_interaction_requested_approval_for_another_approver_is_three_read_only_items() {
    let directory = tempfile::tempdir().expect("tempdir");
    let broker = tap_broker(directory.path()).await;
    let mut rig = Rig::with_broker(Some(broker));
    let row = expectation(
        &read_only_frames("req-3", "Approval pending: Run it"),
        base_state(),
    );
    tap_event(
        &mut rig,
        "interaction_requested_approval_not_approver",
        ROW_REQUESTED,
        requested(PendingInteraction::Approval {
            approver: other_approver(),
            request: Box::new(command_approval("req-3")),
        }),
        &row,
    );
}

#[test]
fn node_tap_interaction_requested_question_without_broker_is_three_read_only_items() {
    let mut rig = Rig::new();
    let row = expectation(
        &read_only_frames("q-2", "Question pending: Which one?"),
        base_state(),
    );
    tap_event(
        &mut rig,
        "interaction_requested_question_not_approver",
        ROW_REQUESTED,
        requested(PendingInteraction::Question {
            approver: tap_actor(),
            request: Box::new(question("q-2")),
        }),
        &row,
    );
}

#[tokio::test]
async fn node_tap_interaction_resolved_pending_request_emits_one_resolved() {
    let directory = tempfile::tempdir().expect("tempdir");
    let broker = tap_broker(directory.path()).await;
    let mut rig = Rig::with_broker(Some(broker));
    rig.setup(requested(PendingInteraction::Approval {
        approver: tap_actor(),
        request: Box::new(command_approval("req-1")),
    }));
    let row = expectation(
        &[frame(
            "serverRequest/resolved",
            json!({"requestId":"router:approval:tap-session:req-1"}),
        )],
        base_state(),
    );
    tap_event(
        &mut rig,
        "interaction_resolved_pending",
        ROW_RESOLVED,
        SessionEvent::InteractionResolved {
            request_id: "req-1".into(),
        },
        &row,
    );
}

#[test]
fn node_tap_interaction_resolved_without_pending_request_emits_nothing() {
    let mut rig = Rig::new();
    let row = expectation(&[], base_state());
    tap_event(
        &mut rig,
        "interaction_resolved_unknown",
        ROW_RESOLVED,
        SessionEvent::InteractionResolved {
            request_id: "nobody".into(),
        },
        &row,
    );
}

#[tokio::test]
async fn node_tap_interaction_resolved_emits_one_resolved_per_pending_id_including_retries() {
    let directory = tempfile::tempdir().expect("tempdir");
    let broker = tap_broker(directory.path()).await;
    let mut rig = Rig::with_broker(Some(Arc::clone(&broker)));
    let pending_question = question("q-1");
    let _agent_reply = broker
        .request_question(
            rig.session.clone(),
            tap_actor(),
            pending_question.clone(),
            None,
        )
        .await
        .expect("question registered");
    let present = PendingInteraction::Question {
        approver: tap_actor(),
        request: Box::new(pending_question),
    };
    let first = rig.setup(requested(present.clone()));
    let first_id = first[0]["id"].as_str().expect("request id").to_owned();
    // A malformed answer is rejected and re-asked under a fresh retry id.
    let retried = rig
        .forwarding
        .resolve_reply(&json!({"id":first_id,"result":{"action":"accept","content":{}}}))
        .await;
    assert_eq!(retried.len(), 2, "error plus retried request: {retried:?}");
    // The provider presents the request again: the original id is pending next to the retry id.
    rig.setup(requested(present));
    let row = expectation(
        &[
            frame(
                "serverRequest/resolved",
                json!({"requestId":"<volatile:requestId>"}),
            ),
            frame(
                "serverRequest/resolved",
                json!({"requestId":"<volatile:requestId>"}),
            ),
        ],
        base_state(),
    );
    // The pending ids come out of a hash map: the recording sorts the frames by request id. The
    // retry id is generated inside the request id, so the compared copy replaces the request id
    // and the exact ids are asserted below against the raw frames.
    let frames = tap_event_normalized(
        &mut rig,
        "interaction_resolved_per_pending_id_with_retries",
        ROW_RESOLVED,
        SessionEvent::InteractionResolved {
            request_id: "q-1".into(),
        },
        &row,
        |frames| frames.sort_by_key(|frame| frame["params"]["requestId"].to_string()),
        &["startedAtMs", "completedAtMs", "requestId"],
    );
    let mut resolved_ids: Vec<&str> = frames
        .iter()
        .filter_map(|frame| frame["params"]["requestId"].as_str())
        .collect();
    resolved_ids.sort_unstable();
    assert_eq!(resolved_ids.len(), 2, "{frames:?}");
    assert_eq!(resolved_ids[0], "router:question:tap-session:q-1");
    assert!(
        resolved_ids[1]
            .strip_prefix("router:question:tap-session:q-1:retry:")
            .is_some_and(|retry_id| !retry_id.is_empty()),
        "{frames:?}"
    );
}
