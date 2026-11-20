use serde_json::{Value, json};
use std::collections::BTreeMap;
use vendor_protocol::codex::{
    AdditionalContextEntry, AdditionalContextKind, CodexErrorInfo, CodexRequest, ReasoningSummary,
    TextUserInput, TurnStartParams, TurnStartResponse, TurnStatus, UserInput,
};

const MINIMAL_REQUEST: &str = r#"{"threadId":"thread-fixture","input":[]}"#;

#[test]
fn constructs_the_typed_turn_request_with_tagged_input_and_context() {
    let base: TurnStartParams = serde_json::from_str(MINIMAL_REQUEST).expect("minimal request");
    let request = TurnStartParams {
        input: vec![UserInput::Text(Box::new(TextUserInput {
            text: "fixture prompt".into(),
            text_elements: None,
        }))],
        additional_context: Some(Some(BTreeMap::from([(
            "app-fixture".into(),
            AdditionalContextEntry {
                kind: AdditionalContextKind::Application,
                name: Some(None),
                value: "fixture context".into(),
            },
        )]))),
        ..base
    };
    let wire = json!({
        "threadId": "thread-fixture",
        "input": [{"type": "text", "text": "fixture prompt"}],
        "additionalContext": {
            "app-fixture": {"kind": "application", "name": null, "value": "fixture context"}
        }
    });

    let decoded: TurnStartParams =
        serde_json::from_value(wire.clone()).expect("literal turn request");

    assert_eq!(TurnStartParams::METHOD, "turn/start");
    assert_eq!(request, decoded);
    assert_eq!(serde_json::to_value(request).expect("request JSON"), wire);
}

#[test]
fn turn_request_rejects_missing_top_level_fields_and_malformed_known_values() {
    for malformed in [
        r#"{"input":[]}"#,
        r#"{"threadId":"thread-fixture"}"#,
        r#"{"threadId":"thread-fixture","input":null}"#,
        r#"{"threadId":"thread-fixture","input":[{"type":{"text":null},"text":"fixture"}]}"#,
        r#"{"threadId":"thread-fixture","input":[{"type":"text","text":2}]}"#,
        r#"{"threadId":"thread-fixture","input":[],"additionalContext":{"app":{"kind":"application"}}}"#,
        r#"{"threadId":"thread-fixture","input":[],"additionalContext":{"app":{"kind":1,"value":"fixture"}}}"#,
    ] {
        assert!(
            serde_json::from_str::<TurnStartParams>(malformed).is_err(),
            "{malformed}"
        );
    }
}

#[test]
fn turn_request_retains_omitted_null_and_empty_values_and_raw_output_schema() {
    const OUTPUT_SCHEMA: &str =
        r#"{"const":18446744073709551617,"multipleOf":0.12345678901234567890123456789}"#;
    let omitted: TurnStartParams = serde_json::from_str(MINIMAL_REQUEST).expect("omitted fields");
    let explicit: TurnStartParams = serde_json::from_str(
        r#"{"threadId":"thread-fixture","input":[],"environments":[],"outputSchema":null}"#,
    )
    .expect("explicit values");
    let present_wire = format!(
        r#"{{"threadId":"thread-fixture","input":[],"environments":null,"outputSchema":{OUTPUT_SCHEMA}}}"#
    );
    let present: TurnStartParams =
        serde_json::from_str(&present_wire).expect("opaque output schema");

    assert_eq!(omitted.environments, None);
    assert_eq!(omitted.output_schema, None);
    assert_eq!(
        serde_json::to_value(omitted).expect("omitted JSON"),
        json!({"threadId":"thread-fixture","input":[]})
    );
    assert_eq!(explicit.environments, Some(Some(Vec::new())));
    assert_eq!(
        explicit
            .output_schema
            .as_ref()
            .expect("present null")
            .as_json(),
        "null"
    );
    assert_eq!(
        serde_json::to_value(explicit).expect("explicit JSON"),
        json!({"threadId":"thread-fixture","input":[],"environments":[],"outputSchema":null})
    );
    assert_eq!(present.environments, Some(None));
    assert_eq!(
        present
            .output_schema
            .as_ref()
            .expect("present opaque value")
            .as_json(),
        OUTPUT_SCHEMA
    );
    assert!(
        serde_json::to_string(&present)
            .expect("serialized wire")
            .contains(&format!(r#""outputSchema":{OUTPUT_SCHEMA}"#))
    );
}

#[test]
fn turn_summary_accepts_only_the_four_pinned_string_tags() {
    for (tag, expected) in [
        ("auto", ReasoningSummary::Auto),
        ("concise", ReasoningSummary::Concise),
        ("detailed", ReasoningSummary::Detailed),
        ("none", ReasoningSummary::None),
    ] {
        let wire = format!(r#"{{"threadId":"thread-fixture","input":[],"summary":"{tag}"}}"#);
        let request: TurnStartParams = serde_json::from_str(&wire).expect("pinned summary");

        assert_eq!(request.summary, Some(Some(expected)));
        let reencoded: Value = serde_json::to_value(request).expect("summary JSON");
        assert_eq!(reencoded.get("summary").and_then(Value::as_str), Some(tag));
    }
    for bad_summary in [r#"{"none":null}"#, r#""adaptive""#, "[]"] {
        let wire = format!(r#"{{"threadId":"thread-fixture","input":[],"summary":{bad_summary}}}"#);
        assert!(
            serde_json::from_str::<TurnStartParams>(&wire).is_err(),
            "{wire}"
        );
    }
}

#[test]
fn associated_turn_reply_uses_the_typed_status_and_keeps_opaque_error_detail() {
    let wire = br#"{"turn":{"id":"turn-fixture","items":[],"status":"failed","error":{"message":"fixture","codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":"malformed","counter":18446744073709551617}}}}}"#;

    let reply: TurnStartResponse =
        TurnStartParams::decode_response(wire).expect("associated reply");

    assert_eq!(reply.turn.status, TurnStatus::Failed);
    let Some(CodexErrorInfo::MalformedKnownObject(detail)) = reply
        .turn
        .error
        .flatten()
        .expect("turn error")
        .codex_error_info
        .flatten()
    else {
        panic!("expected malformed-known opaque detail");
    };
    assert_eq!(
        detail.as_json(),
        r#"{"httpConnectionFailed":{"httpStatusCode":"malformed","counter":18446744073709551617}}"#
    );
    assert!(TurnStartParams::decode_response(br#"{}"#).is_err());
    assert!(
        TurnStartParams::decode_response(
            br#"{"turn":{"id":"turn-fixture","items":[],"status":{"failed":null}}}"#
        )
        .is_err()
    );
}

#[test]
fn turn_start_request_cannot_decode_a_thread_read_response_type() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/integration/codex_turn_start/wrong_reply.rs");
}
