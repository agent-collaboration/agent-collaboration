use serde::Deserialize;
use serde_json::value::RawValue;
use vendor_protocol::codex::{
    AgentMessageDeltaNotification, CodexErrorInfo, CodexServerNotification,
    ItemCompletedNotification, ItemStartedNotification, ThreadItem, TurnCompletedNotification,
    TurnStartedNotification, TurnStatus,
};

const TURN_STARTED: &[u8] = br#"{"jsonrpc":"2.0","method":"turn/started","params":{"threadId":"thread-fixture","turn":{"id":"turn-fixture","items":[],"status":"inProgress"}}}"#;

#[derive(Deserialize)]
struct RawNotification<'wire> {
    method: String,
    #[serde(borrow)]
    params: &'wire RawValue,
}

fn decode_notification<Notification: CodexServerNotification>(
    wire: &[u8],
) -> serde_json::Result<Notification> {
    let envelope: RawNotification<'_> = serde_json::from_slice(wire)?;
    if envelope.method != Notification::METHOD {
        return Err(serde::de::Error::custom(format_args!(
            "expected method {}, found {}",
            Notification::METHOD,
            envelope.method
        )));
    }
    Notification::decode_params(envelope.params.get().as_bytes())
}

#[test]
fn turn_started_uses_its_method_even_when_completed_has_the_same_payload_shape() {
    let started = decode_notification::<TurnStartedNotification>(TURN_STARTED)
        .expect("literal turn/started notification");

    assert_eq!(TurnStartedNotification::METHOD, "turn/started");
    assert_eq!(TurnCompletedNotification::METHOD, "turn/completed");
    assert_eq!(started.thread_id, "thread-fixture");
    assert_eq!(started.turn.id, "turn-fixture");
    assert_eq!(started.turn.status, TurnStatus::InProgress);
    let wrong_method = decode_notification::<TurnCompletedNotification>(TURN_STARTED)
        .expect_err("a matching payload shape cannot select a different notification");
    assert_eq!(
        wrong_method.to_string(),
        "expected method turn/completed, found turn/started"
    );
}

#[test]
fn turn_completed_preserves_terminal_status_and_opaque_error_tokens() {
    let raw_detail = r#"{"httpConnectionFailed":{"httpStatusCode":"malformed","counter":18446744073709551617,"detail":0.12345678901234567890123456789}}"#;
    let wire = format!(
        r#"{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"thread-fixture","turn":{{"id":"turn-fixture","items":[],"status":"failed","error":{{"message":"fixture","codexErrorInfo":{raw_detail}}}}}}}}}"#,
    );

    let completed =
        decode_notification::<TurnCompletedNotification>(wire.as_bytes()).expect("terminal event");

    assert_eq!(completed.thread_id, "thread-fixture");
    assert_eq!(completed.turn.id, "turn-fixture");
    assert_eq!(completed.turn.status, TurnStatus::Failed);
    let detail = completed
        .turn
        .error
        .as_ref()
        .and_then(Option::as_ref)
        .expect("turn error");
    assert_eq!(detail.message, "fixture");
    let Some(CodexErrorInfo::MalformedKnownObject(detail)) =
        detail.codex_error_info.as_ref().and_then(Option::as_ref)
    else {
        panic!("expected the malformed-known opaque error");
    };
    assert_eq!(detail.as_json(), raw_detail);
    let encoded = serde_json::to_string(&completed).expect("terminal event params");
    assert!(encoded.contains(&format!(r#""codexErrorInfo":{raw_detail}"#)));
    println!("turn/completed input: {wire}");
    println!("turn/completed output params: {encoded}");
}

#[test]
fn item_events_preserve_typed_items_and_the_exact_millisecond_fields() {
    let started_wire = br#"{"method":"item/started","params":{"threadId":"thread-fixture","turnId":"turn-fixture","startedAtMs":1700000000123,"item":{"type":"agentMessage","id":"item-start-fixture","text":"fixture"}}}"#;
    let completed_wire = br#"{"method":"item/completed","params":{"threadId":"thread-fixture","turnId":"turn-fixture","completedAtMs":1700000000456,"item":{"type":"mcpToolCall","id":"item-completed-fixture","server":"fixture","tool":"fixture","status":"completed","arguments":{"counter":18446744073709551617}}}}"#;

    let started =
        decode_notification::<ItemStartedNotification>(started_wire).expect("item started");
    let completed =
        decode_notification::<ItemCompletedNotification>(completed_wire).expect("item completed");

    assert_eq!(ItemStartedNotification::METHOD, "item/started");
    assert_eq!(ItemCompletedNotification::METHOD, "item/completed");
    assert_eq!(started.started_at_ms, 1_700_000_000_123);
    assert_eq!(completed.completed_at_ms, 1_700_000_000_456);
    assert_eq!(
        (started.thread_id.as_str(), started.turn_id.as_str()),
        ("thread-fixture", "turn-fixture")
    );
    assert_eq!(
        (completed.thread_id.as_str(), completed.turn_id.as_str()),
        ("thread-fixture", "turn-fixture")
    );
    let ThreadItem::AgentMessage(message) = &started.item else {
        panic!("expected the agent message item");
    };
    assert_eq!(message.id, "item-start-fixture");
    assert_eq!(message.text, "fixture");
    let ThreadItem::McpToolCall(call) = &completed.item else {
        panic!("expected the MCP tool call item");
    };
    assert_eq!(call.id, "item-completed-fixture");
    assert_eq!(
        call.arguments.as_json(),
        r#"{"counter":18446744073709551617}"#
    );

    let started_params = serde_json::to_string(&started).expect("started params");
    let completed_params = serde_json::to_string(&completed).expect("completed params");
    assert!(started_params.contains(r#""startedAtMs":1700000000123"#));
    assert!(completed_params.contains(r#""completedAtMs":1700000000456"#));
    assert!(completed_params.contains(r#""arguments":{"counter":18446744073709551617}"#));
    println!(
        "item/started input: {}",
        std::str::from_utf8(started_wire).expect("literal JSON")
    );
    println!("item/started output params: {started_params}");
    println!(
        "item/completed input: {}",
        std::str::from_utf8(completed_wire).expect("literal JSON")
    );
    println!("item/completed output params: {completed_params}");
}

#[test]
fn agent_message_delta_uses_its_string_and_three_wire_identities() {
    let wire = br#"{"method":"item/agentMessage/delta","params":{"threadId":"thread-fixture","turnId":"turn-fixture","itemId":"item-fixture","delta":"one\ntwo \u03bb"}}"#;

    let delta = decode_notification::<AgentMessageDeltaNotification>(wire).expect("literal delta");

    assert_eq!(
        AgentMessageDeltaNotification::METHOD,
        "item/agentMessage/delta"
    );
    assert_eq!(delta.thread_id, "thread-fixture");
    assert_eq!(delta.turn_id, "turn-fixture");
    assert_eq!(delta.item_id, "item-fixture");
    assert_eq!(delta.delta, "one\ntwo λ");
}

#[test]
fn notification_decoders_reject_missing_and_malformed_known_fields() {
    let missing_turn = br#"{"threadId":"thread-fixture"}"#;
    assert!(TurnStartedNotification::decode_params(missing_turn).is_err());
    assert!(TurnCompletedNotification::decode_params(missing_turn).is_err());
    assert!(TurnCompletedNotification::decode_params(
        br#"{"threadId":"thread-fixture","turn":{"id":"turn-fixture","items":[],"status":{"failed":null}}}"#,
    ).is_err());

    assert!(ItemStartedNotification::decode_params(
        br#"{"threadId":"thread-fixture","turnId":"turn-fixture","startedAt":1700000000123,"item":{"type":"agentMessage","id":"item-fixture","text":"fixture"}}"#,
    ).is_err());
    assert!(ItemStartedNotification::decode_params(
        br#"{"threadId":"thread-fixture","turnId":"turn-fixture","startedAtMs":1700000000123,"item":{"type":"agentMessage","id":"item-fixture","text":2}}"#,
    ).is_err());
    assert!(ItemCompletedNotification::decode_params(
        br#"{"threadId":"thread-fixture","turnId":"turn-fixture","completedAtMs":"1700000000456","item":{"type":"agentMessage","id":"item-fixture","text":"fixture"}}"#,
    ).is_err());
    assert!(ItemCompletedNotification::decode_params(
        br#"{"threadId":"thread-fixture","turnId":"turn-fixture","item":{"type":"agentMessage","id":"item-fixture","text":"fixture"}}"#,
    ).is_err());

    for invalid in [
        br#"{"threadId":"thread-fixture","turnId":"turn-fixture","delta":"text"}"#.as_slice(),
        br#"{"threadId":"thread-fixture","turnId":"turn-fixture","itemId":"item-fixture","delta":2}"#,
        br#"{"threadId":"thread-fixture","turnId":"turn-fixture","itemId":"item-fixture","delta":"text"} {}"#,
    ] {
        assert!(AgentMessageDeltaNotification::decode_params(invalid).is_err());
    }
}

#[test]
fn equal_shaped_turn_notifications_cannot_be_interchanged() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/integration/codex_observation_bindings/wrong_notification.rs");
}
