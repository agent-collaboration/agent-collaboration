use serde::Deserialize;
use serde_json::{Value, value::RawValue};
use vendor_protocol::codex::{
    ApprovalsReviewer, CodexRequest, ModelListParams, ModelListResponse, SandboxPolicy, ThreadItem,
    ThreadReadParams, ThreadReadResponse, ThreadResumeParams, ThreadResumeResponse, ThreadStatus,
    TurnStatus,
};

// The reviewer's valid minimal thread, including a tagged status and no opaque leaves.
const MINIMAL_THREAD: &str = r#"{"cliVersion":"0.166.0","createdAt":0,"cwd":"/fixture","ephemeral":false,"id":"t","modelProvider":"p","preview":"","projectId":null,"sessionId":"s","source":"cli","status":{"type":"idle"},"turns":[],"updatedAt":1}"#;
const TURNS_WITH_WIDE_ARGUMENT: &str = r#"[{"id":"turn","status":"completed","items":[{"type":"mcpToolCall","id":"i","server":"s","tool":"t","status":"completed","arguments":{"counter":18446744073709551617}}]}]"#;

#[derive(Deserialize)]
struct RawReply<'wire> {
    #[serde(borrow)]
    result: &'wire RawValue,
}

fn decode_reply<Request: CodexRequest>(envelope: &[u8]) -> serde_json::Result<Request::Response> {
    let reply: RawReply<'_> = serde_json::from_slice(envelope)?;
    Request::decode_response(reply.result.get().as_bytes())
}

#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "The literal reply contains one MCP tool call."
)]
fn argument_json(reply: &ThreadReadResponse) -> &str {
    let turn = reply.thread.turns.first().expect("terminal turn");
    assert_eq!(turn.status, TurnStatus::Completed);
    let Some(ThreadItem::McpToolCall(item)) = turn.items.first() else {
        panic!("expected the typed MCP tool call");
    };
    item.arguments.as_json()
}

#[test]
fn raw_thread_read_result_retains_an_opaque_number_that_value_ingress_rounds() {
    let thread = MINIMAL_THREAD.replace(
        r#""turns":[]"#,
        &format!(r#""turns":{TURNS_WITH_WIDE_ARGUMENT}"#),
    );
    let envelope = format!(r#"{{"jsonrpc":"2.0","id":"read-1","result":{{"thread":{thread}}}}}"#);

    let direct: ThreadReadResponse =
        decode_reply::<ThreadReadParams>(envelope.as_bytes()).expect("raw thread/read reply");
    let generic: Value = serde_json::from_str(&envelope).expect("whole envelope as Value");
    let rounded: ThreadReadResponse =
        serde_json::from_value(generic.get("result").expect("result field").clone())
            .expect("thread/read reply from Value");

    let exact_arguments = argument_json(&direct);
    let rounded_arguments = argument_json(&rounded);
    assert_eq!(exact_arguments, r#"{"counter":18446744073709551617}"#);
    assert_eq!(rounded_arguments, r#"{"counter":1.8446744073709552e+19}"#);
    assert_eq!(direct.thread.id, rounded.thread.id);
    println!("raw result arguments: {exact_arguments}");
    println!("Value ingress arguments: {rounded_arguments}");
}

#[test]
fn raw_thread_resume_result_decodes_to_the_associated_response() {
    let envelope = format!(
        r#"{{"jsonrpc":"2.0","id":"resume-1","result":{{"thread":{MINIMAL_THREAD},"approvalPolicy":"on-request","approvalsReviewer":"user","cwd":"/fixture","model":"fixture-model","modelProvider":"p","sandbox":{{"type":"workspaceWrite"}},"runtimeWorkspaceRoots":["/fixture"]}}}}"#
    );

    let reply: ThreadResumeResponse =
        decode_reply::<ThreadResumeParams>(envelope.as_bytes()).expect("raw thread/resume reply");

    assert!(matches!(reply.sandbox, SandboxPolicy::WorkspaceWrite(_)));
    assert_eq!(reply.approvals_reviewer, ApprovalsReviewer::User);
    assert_eq!(reply.runtime_workspace_roots(), ["/fixture"]);
    assert_eq!(reply.thread.id, "t");
}

#[test]
fn raw_model_list_result_decodes_to_the_associated_response() {
    let envelope = br#"{"jsonrpc":"2.0","id":"model-list-1","result":{"data":[{"id":"fixture-model","model":"fixture-model","displayName":"Fixture model","description":"","hidden":false,"isDefault":true,"defaultReasoningEffort":"standard","supportedReasoningEfforts":[]}],"nextCursor":"next"}}"#;

    let reply: ModelListResponse =
        decode_reply::<ModelListParams>(envelope).expect("raw model/list reply");

    assert_eq!(reply.data.first().expect("model").id, "fixture-model");
    assert_eq!(
        reply.next_cursor.as_ref().and_then(Option::as_deref),
        Some("next")
    );
}

#[test]
fn response_decoder_rejects_a_malformed_typed_field_and_trailing_input() {
    let invalid_thread = MINIMAL_THREAD.replace(r#""type":"idle""#, r#""type":{"idle":null}"#);
    let envelope =
        format!(r#"{{"jsonrpc":"2.0","id":"read-1","result":{{"thread":{invalid_thread}}}}}"#);

    let invalid = decode_reply::<ThreadReadParams>(envelope.as_bytes())
        .expect_err("the status tag must be a string");
    assert!(invalid.is_data(), "{invalid}");

    let trailing = ModelListParams::decode_response(br#"{"data":[]} true"#)
        .expect_err("the decoder must consume the whole payload");
    assert!(trailing.is_syntax(), "{trailing}");
    assert!(trailing.to_string().contains("trailing characters"));
    assert!(ModelListParams::decode_response(b"{\"data\":[]} \n").is_ok());
}

#[test]
fn serde_buffered_envelopes_can_reject_the_valid_minimal_read_fixture() {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum UntaggedEnvelope {
        Reply {
            #[serde(rename = "result")]
            _result: ThreadReadResponse,
        },
    }
    #[derive(Deserialize)]
    struct FlattenedEnvelope {
        #[serde(flatten)]
        _result: ThreadReadResponse,
    }
    #[derive(Deserialize)]
    #[serde(tag = "kind", rename_all = "camelCase")]
    enum InternallyTaggedEnvelope {
        Read(ThreadReadResponse),
    }

    let envelope =
        format!(r#"{{"jsonrpc":"2.0","id":"read-1","result":{{"thread":{MINIMAL_THREAD}}}}}"#);
    let flat = format!(r#"{{"jsonrpc":"2.0","id":"read-1","thread":{MINIMAL_THREAD}}}"#);
    let tagged = format!(r#"{{"kind":"read","thread":{MINIMAL_THREAD}}}"#);

    let valid = decode_reply::<ThreadReadParams>(envelope.as_bytes()).expect("raw result");
    assert!(matches!(valid.thread.status, ThreadStatus::Idle(_)));
    let untagged = serde_json::from_str::<UntaggedEnvelope>(&envelope)
        .err()
        .expect("untagged buffer");
    let flattened = serde_json::from_str::<FlattenedEnvelope>(&flat)
        .err()
        .expect("flatten buffer");
    let internally_tagged = serde_json::from_str::<InternallyTaggedEnvelope>(&tagged)
        .map(|reply| match reply {
            InternallyTaggedEnvelope::Read(body) => body,
        })
        .expect_err("internally tagged buffer");

    assert!(untagged.to_string().contains("did not match any variant"));
    assert!(
        flattened
            .to_string()
            .contains("invalid type: newtype struct")
    );
    assert!(
        internally_tagged
            .to_string()
            .contains("invalid type: newtype struct")
    );
    println!("untagged: {untagged}");
    println!("flattened: {flattened}");
    println!("internally tagged: {internally_tagged}");
}
