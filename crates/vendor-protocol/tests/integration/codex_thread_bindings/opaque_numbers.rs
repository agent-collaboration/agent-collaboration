use serde_json::Value;
use vendor_protocol::codex::{CodexErrorInfo, ThreadReadResponse, ThreadResumeParams, TurnStatus};

use super::thread_fixture;

const WIDE_INTEGER: &str = "18446744073709551617";
const WIDE_DECIMAL: &str = "-0.123456789012345678901234567890";

fn read_wire(turn: &str) -> String {
    let thread = thread_fixture().to_string();
    let thread = thread.replace("\"turns\":[]", &format!("\"turns\":[{turn}]"));
    format!(r#"{{"thread":{thread}}}"#)
}

#[expect(
    clippy::expect_used,
    reason = "These literals name a single output token."
)]
fn assert_numeric_token(encoded: &str, key: &str, number: &str) {
    let field = format!(r#""{key}":"#);
    let (_, remainder) = encoded.split_once(&field).expect("serialized field");
    let token = remainder
        .trim_start()
        .split([',', '}', ']'])
        .next()
        .expect("serialized numeric token");
    assert_eq!(token.trim_end(), number, "{key} in {encoded}");
}

#[expect(
    clippy::expect_used,
    reason = "This literal fixture contains a terminal error."
)]
fn terminal_error(result: &ThreadReadResponse) -> &CodexErrorInfo {
    let turn = result.thread.turns.first().expect("terminal turn");
    assert_eq!(turn.status, TurnStatus::Failed);
    let error = turn
        .error
        .as_ref()
        .and_then(Option::as_ref)
        .expect("turn error");
    assert_eq!(error.message, "fixture error");
    error
        .codex_error_info
        .as_ref()
        .and_then(Option::as_ref)
        .expect("error detail")
}

fn terminal_wire(details: &str) -> String {
    read_wire(&format!(
        r#"{{"id":"turn-1","status":"failed","items":[],"error":{{"message":"fixture error","codexErrorInfo":{details}}}}}"#
    ))
}

#[test]
fn future_error_object_keeps_its_exact_nested_number() {
    let wire = terminal_wire(&format!(
        r#"{{"futureError":{{"counter":{WIDE_INTEGER}}}}}"#
    ));

    let typed: ThreadReadResponse = serde_json::from_str(&wire).expect("future error reply");
    let encoded = serde_json::to_string(&typed).expect("serialize reply");

    assert!(matches!(
        terminal_error(&typed),
        CodexErrorInfo::UnknownObject(_)
    ));
    assert_numeric_token(&encoded, "counter", WIDE_INTEGER);
}

#[test]
fn malformed_known_error_object_keeps_its_exact_nested_number() {
    let wire = terminal_wire(&format!(
        r#"{{"httpConnectionFailed":{{"httpStatusCode":"malformed","counter":{WIDE_INTEGER}}}}}"#
    ));

    let typed: ThreadReadResponse =
        serde_json::from_slice(wire.as_bytes()).expect("malformed detail reply");
    let encoded = serde_json::to_string(&typed).expect("serialize reply");

    assert!(matches!(
        terminal_error(&typed),
        CodexErrorInfo::MalformedKnownObject(_)
    ));
    assert_numeric_token(&encoded, "counter", WIDE_INTEGER);
    let known: ThreadReadResponse = serde_json::from_str(&terminal_wire(
        r#"{"httpConnectionFailed":{"httpStatusCode":302}}"#,
    ))
    .expect("typed error reply");
    assert!(matches!(
        terminal_error(&known),
        CodexErrorInfo::HttpConnectionFailed(_)
    ));
}

#[test]
fn resume_configuration_keeps_wide_integers_and_decimals() {
    let wire = format!(
        r#"{{"threadId":"fixture-thread","config":{{"nested":{{"counter":{WIDE_INTEGER},"decimal":{WIDE_DECIMAL}}}}}}}"#
    );

    let typed: ThreadResumeParams = serde_json::from_str(&wire).expect("resume params");
    let encoded = serde_json::to_string(&typed).expect("serialize params");

    assert_eq!(typed.thread_id, "fixture-thread");
    assert_numeric_token(&encoded, "counter", WIDE_INTEGER);
    assert_numeric_token(&encoded, "decimal", WIDE_DECIMAL);
}

#[test]
fn tagged_mcp_tool_arguments_keep_a_wide_number() {
    let wire = read_wire(&format!(
        r#"{{"id":"turn-1","status":"completed","items":[{{"type":"mcpToolCall","id":"item","server":"fixture","tool":"fixture","status":"completed","arguments":{{"counter":{WIDE_INTEGER}}}}}]}}"#
    ));

    let typed: ThreadReadResponse = serde_json::from_str(&wire).expect("MCP reply");
    let encoded = serde_json::to_string(&typed).expect("serialize reply");

    assert_eq!(
        typed.thread.turns.first().expect("turn").status,
        TurnStatus::Completed
    );
    assert_numeric_token(&encoded, "counter", WIDE_INTEGER);
}

#[test]
fn other_schema_open_leaves_keep_numbers_through_nested_shapes() {
    let wire = format!(
        r#"{{"threadId":"fixture-thread","history":[{{"type":"additional_tools","role":"fixture","tools":[{{"tools_counter":{WIDE_INTEGER}}}]}},{{"type":"function_call_output","output":[{{"type":"input_image","file_id":"file-fixture","image_url":{{"bad_alternative_counter":{WIDE_INTEGER}}}}}]}}]}}"#
    );

    let typed: ThreadResumeParams = serde_json::from_str(&wire).expect("resume history");
    let encoded = serde_json::to_string(&typed).expect("serialize history");

    assert_numeric_token(&encoded, "tools_counter", WIDE_INTEGER);
    assert_numeric_token(&encoded, "bad_alternative_counter", WIDE_INTEGER);

    let wire = read_wire(&format!(
        r#"{{"id":"turn-1","status":"completed","items":[{{"type":"mcpToolCall","id":"item","server":"fixture","tool":"fixture","status":"completed","arguments":null,"result":{{"content":[{{"content_counter":{WIDE_INTEGER}}}],"_meta":{{"metadata_counter":{WIDE_INTEGER}}}}}}}]}}"#
    ));
    let typed: ThreadReadResponse = serde_json::from_str(&wire).expect("reply with MCP result");
    let encoded = serde_json::to_string(&typed).expect("serialize reply");
    assert_numeric_token(&encoded, "content_counter", WIDE_INTEGER);
    assert_numeric_token(&encoded, "metadata_counter", WIDE_INTEGER);
    assert!(encoded.contains(r#""arguments":null"#));

    let thread = thread_fixture().to_string();
    let wire = format!(r#"{{"thread":{thread}}}"#).replace(
        "\"ephemeral\":false",
        &format!(r#""ephemeral":false,"extra":{{"extra_counter":{WIDE_INTEGER}}}"#),
    );
    let typed: ThreadReadResponse = serde_json::from_str(&wire).expect("thread with opaque extra");
    assert_numeric_token(
        &serde_json::to_string(&typed).expect("serialize extra"),
        "extra_counter",
        WIDE_INTEGER,
    );
}

#[test]
fn typed_agent_state_map_still_rejects_untyped_values() {
    let item = |state: &Value| {
        serde_json::json!({
            "type":"collabAgentToolCall","id":"item","senderThreadId":"sender",
            "receiverThreadIds":["receiver"],"status":"completed","tool":"wait",
            "agentsStates":{"receiver":state}
        })
    };
    let valid = item(&serde_json::json!({"status":"completed"}));
    let valid = format!(r#"{{"id":"turn-1","status":"completed","items":[{valid}]}}"#);
    assert!(serde_json::from_str::<ThreadReadResponse>(&read_wire(&valid)).is_ok());

    for malformed in [
        serde_json::json!({}),
        serde_json::json!({"status":1}),
        Value::Null,
    ] {
        let invalid = item(&malformed);
        let invalid = format!(r#"{{"id":"turn-1","status":"completed","items":[{invalid}]}}"#);
        assert!(serde_json::from_str::<ThreadReadResponse>(&read_wire(&invalid)).is_err());
    }
}
