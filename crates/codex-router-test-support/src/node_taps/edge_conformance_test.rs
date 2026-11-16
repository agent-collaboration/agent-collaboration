//! OB2: the checker tells right from wrong, against the real pinned definitions.
use serde_json::{Value, json};

use super::{
    BodyRoot, CheckInput, ContractOwner, EdgeConformance, EdgeDefinitions, MissingContext,
    OwnedCodecs, PinError, RecordLayer, RouterContract, Support, family_edge,
    pinned_schema_directory, sender_role,
};
use crate::node_taps::{
    AcpPeer, ConnectionId, ConnectionLedger, MessageFamily, ProtocolNode, TapDirection,
};

use MessageFamily::{
    AcpToAgent, CodexClientRequestFromCli, CodexMessageToCli, CodexRequestToCodex,
};
use TapDirection::{Consumed, Produced};

fn definitions() -> &'static EdgeDefinitions {
    EdgeDefinitions::shared()
}

/// Checks `raw` as one record of `family`, through `ledger`.
fn check_with(
    ledger: &mut ConnectionLedger,
    codecs: &OwnedCodecs,
    connection: &str,
    layer: &RecordLayer,
    family: MessageFamily,
    direction: TapDirection,
    raw: &Value,
) -> EdgeConformance {
    let (edge, part_is_server) =
        family_edge(family, ProtocolNode::N8CodexAcpAdapter).expect("an outside family");
    let connection = ConnectionId(connection.to_owned());
    definitions().check(
        &CheckInput {
            edge,
            connection: &connection,
            sender: sender_role(part_is_server, direction),
            layer,
            answers: None,
            raw,
            codecs,
        },
        ledger,
    )
}

/// Checks a sequence of `(family, direction, wire)` frames on one connection.
fn check_all(messages: &[(MessageFamily, TapDirection, Value)]) -> Vec<EdgeConformance> {
    check_all_on(None, messages)
}

/// [`check_all`] on a connection whose ACP agent is declared as `peer`.
fn check_all_on(
    peer: Option<AcpPeer>,
    messages: &[(MessageFamily, TapDirection, Value)],
) -> Vec<EdgeConformance> {
    let mut ledger = ConnectionLedger::default();
    if let Some(peer) = peer {
        ledger.declare_peer(&ConnectionId("case".to_owned()), peer);
    }
    let codecs = OwnedCodecs::default();
    messages
        .iter()
        .map(|(family, direction, wire)| {
            check_with(
                &mut ledger,
                &codecs,
                "case",
                &RecordLayer::WireFrame,
                *family,
                *direction,
                wire,
            )
        })
        .collect()
}

fn check_one(family: MessageFamily, direction: TapDirection, wire: Value) -> EdgeConformance {
    check_all(&[(family, direction, wire)]).remove(0)
}

fn assert_conforms(result: &EdgeConformance) {
    assert!(
        matches!(result, EdgeConformance::Conforms { .. }),
        "expected a conforming message, got {result:?}"
    );
}

/// The instance paths of a mismatch, or a panic naming what came back instead.
fn mismatch_paths(result: &EdgeConformance) -> Vec<String> {
    match result {
        EdgeConformance::Violations { violations, .. } => violations
            .iter()
            .map(|violation| violation.instance_path.clone())
            .collect(),
        other => panic!("expected a mismatch, got {other:?}"),
    }
}

fn turn_start(params: Value) -> Value {
    json!({"id": 1, "method": "turn/start", "params": params})
}

fn valid_turn_start_params() -> Value {
    json!({"threadId": "thread-1", "input": [{"type": "text", "text": "hello"}]})
}

fn turn_result(status: &str) -> Value {
    json!({"id": 1, "result": {"turn": {"id": "turn-1", "items": [], "status": status}}})
}

#[test]
fn codex_valid_request_result_notification_and_error_conform() {
    let results = check_all(&[
        (
            CodexClientRequestFromCli,
            Consumed,
            turn_start(valid_turn_start_params()),
        ),
        (CodexMessageToCli, Produced, turn_result("inProgress")),
        (
            CodexMessageToCli,
            Produced,
            json!({"method": "turn/started", "params": {"threadId": "thread-1",
                "turn": {"id": "turn-1", "items": [], "status": "inProgress"}}}),
        ),
        (
            CodexClientRequestFromCli,
            Consumed,
            json!({"id": 2, "method": "turn/interrupt", "params": {"threadId": "thread-1", "turnId": "turn-1"}}),
        ),
        (
            CodexMessageToCli,
            Produced,
            json!({"id": 2, "error": {"code": -32000, "message": "no running turn"}}),
        ),
    ]);
    results.iter().for_each(assert_conforms);
}

#[test]
fn codex_missing_required_field_is_a_mismatch_at_params() {
    let result = check_one(
        CodexClientRequestFromCli,
        Consumed,
        turn_start(json!({"input": [{"type": "text", "text": "hello"}]})),
    );
    assert_eq!(mismatch_paths(&result), vec!["/params"]);
}

#[test]
fn codex_wrong_type_is_a_mismatch_at_the_field() {
    let mut params = valid_turn_start_params();
    params["threadId"] = json!(5);
    let result = check_one(CodexClientRequestFromCli, Consumed, turn_start(params));
    assert_eq!(mismatch_paths(&result), vec!["/params/threadId"]);
}

#[test]
fn codex_json_carried_as_a_string_is_a_mismatch() {
    let mut params = valid_turn_start_params();
    params["input"] = json!(r#"[{"type":"text","text":"hello"}]"#);
    let result = check_one(CodexClientRequestFromCli, Consumed, turn_start(params));
    assert_eq!(mismatch_paths(&result), vec!["/params/input"]);
}

#[test]
fn codex_unknown_value_of_a_closed_enum_is_a_mismatch() {
    let results = check_all(&[
        (
            CodexClientRequestFromCli,
            Consumed,
            turn_start(valid_turn_start_params()),
        ),
        (CodexMessageToCli, Produced, turn_result("running")),
    ]);
    assert_conforms(&results[0]);
    assert_eq!(mismatch_paths(&results[1]), vec!["/turn/status"]);
}

#[test]
fn codex_result_paired_to_the_wrong_request_is_a_mismatch() {
    // A turn/start result answering a thread/start request: judged as ThreadStartResponse.
    let results = check_all(&[
        (
            CodexClientRequestFromCli,
            Consumed,
            json!({"id": 1, "method": "thread/start", "params": {}}),
        ),
        (CodexMessageToCli, Produced, turn_result("inProgress")),
    ]);
    assert_conforms(&results[0]);
    match &results[1] {
        EdgeConformance::Violations { definition, .. } => {
            assert_eq!(definition, "#/definitions/v2/ThreadStartResponse");
        }
        other => panic!("expected a mismatch, got {other:?}"),
    }
}

#[test]
fn codex_request_from_the_wrong_side_is_an_unknown_method() {
    // turn/start is a client request; the app-server never sends it.
    let result = check_one(
        CodexMessageToCli,
        Produced,
        turn_start(valid_turn_start_params()),
    );
    assert_eq!(mismatch_paths(&result), vec!["/method"]);
}

#[test]
fn codex_router_as_client_of_managed_codex_sends_client_requests() {
    assert_conforms(&check_one(
        CodexRequestToCodex,
        Produced,
        turn_start(valid_turn_start_params()),
    ));
}

#[test]
fn a_reply_without_its_recorded_request_is_unjudgeable() {
    let result = check_one(CodexMessageToCli, Produced, turn_result("inProgress"));
    assert!(
        matches!(
            result,
            EdgeConformance::Incomplete {
                owner: ContractOwner::Codex,
                missing: MissingContext::OrphanReply { .. }
            }
        ),
        "{result:?}"
    );
}

fn acp_prompt(params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "session/prompt", "params": params})
}

fn valid_prompt_params() -> Value {
    json!({"sessionId": "session-1", "prompt": [{"type": "text", "text": "hello"}]})
}

fn acp_result(id: u64, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

#[test]
fn acp_valid_request_result_and_notification_conform() {
    let results = check_all(&[
        (AcpToAgent, Produced, acp_prompt(valid_prompt_params())),
        (
            MessageFamily::AcpSessionUpdate,
            Consumed,
            json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "session-1",
                "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "hi"}}}}),
        ),
        (
            AcpToAgent,
            Consumed,
            acp_result(1, json!({"stopReason": "end_turn"})),
        ),
    ]);
    results.iter().for_each(assert_conforms);
}

#[test]
fn acp_missing_required_field_and_wrong_type_are_mismatches() {
    let mut missing = valid_prompt_params();
    missing.as_object_mut().expect("object").remove("sessionId");
    assert_eq!(
        mismatch_paths(&check_one(AcpToAgent, Produced, acp_prompt(missing))),
        vec!["/params"]
    );
    let mut wrong_type = valid_prompt_params();
    wrong_type["sessionId"] = json!(5);
    assert_eq!(
        mismatch_paths(&check_one(AcpToAgent, Produced, acp_prompt(wrong_type))),
        vec!["/params/sessionId"]
    );
}

#[test]
fn acp_json_carried_as_a_string_is_a_mismatch() {
    let mut params = valid_prompt_params();
    params["prompt"] = json!(r#"[{"type":"text","text":"hello"}]"#);
    assert_eq!(
        mismatch_paths(&check_one(AcpToAgent, Produced, acp_prompt(params))),
        vec!["/params/prompt"]
    );
}

#[test]
fn acp_unknown_closed_value_and_wrong_pairing_are_mismatches() {
    let results = check_all(&[
        (AcpToAgent, Produced, acp_prompt(valid_prompt_params())),
        (
            AcpToAgent,
            Consumed,
            acp_result(1, json!({"stopReason": "finished"})),
        ),
        (
            AcpToAgent,
            Produced,
            json!({"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {"cwd": "/tmp", "mcpServers": []}}),
        ),
        (
            AcpToAgent,
            Consumed,
            acp_result(2, json!({"stopReason": "end_turn"})),
        ),
    ]);
    assert_eq!(mismatch_paths(&results[1]), vec!["/result/stopReason"]);
    assert_conforms(&results[2]);
    match &results[3] {
        EdgeConformance::Violations { definition, .. } => {
            assert_eq!(definition, "NewSessionResponse:session/new");
        }
        other => panic!("expected a mismatch, got {other:?}"),
    }
}

#[test]
fn acp_missing_jsonrpc_version_is_a_mismatch() {
    let mut message = acp_prompt(valid_prompt_params());
    message.as_object_mut().expect("object").remove("jsonrpc");
    assert_eq!(
        mismatch_paths(&check_one(AcpToAgent, Produced, message)),
        vec![""]
    );
}

#[test]
fn acp_request_from_the_wrong_side_is_an_unknown_method() {
    // session/prompt is a client request; an agent never sends it.
    let result = check_one(AcpToAgent, Consumed, acp_prompt(valid_prompt_params()));
    assert_eq!(mismatch_paths(&result), vec!["/method"]);
}

fn steer(meta: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 3, "method": "_session/steering", "params": {
        "sessionId": "session-1", "prompt": [{"type": "text", "text": "also"}], "_meta": meta}})
}

/// The agent's initialize exchange, advertising steering.
fn steering_initialize() -> [(MessageFamily, TapDirection, Value); 2] {
    [
        (
            AcpToAgent,
            Produced,
            json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": 1}}),
        ),
        (
            AcpToAgent,
            Consumed,
            acp_result(
                0,
                json!({"protocolVersion": 1,
                    "agentInfo": {"name": "@agentclientprotocol/claude-agent-acp", "version": "0.81.0"},
                    "_meta": {"steering": {"supported": true}}}),
            ),
        ),
    ]
}

#[test]
fn claude_steering_is_checked_against_claude_extras() {
    let [init, init_result] = steering_initialize();
    let results = check_all(&[
        init,
        init_result,
        (
            AcpToAgent,
            Produced,
            steer(json!({"steering": {"idleBehavior": "promptRequired"}})),
        ),
        (
            AcpToAgent,
            Consumed,
            acp_result(
                3,
                json!({"outcome": "promptRequired", "reason": "noRunningTurn"}),
            ),
        ),
    ]);
    let results = &results[2..];
    assert!(
        results.iter().all(|result| matches!(
            result,
            EdgeConformance::Conforms {
                owner: ContractOwner::ClaudeExtra,
                ..
            }
        )),
        "{results:?}"
    );
    let [init, init_result] = steering_initialize();
    let wrong = check_all(&[
        init,
        init_result,
        (
            AcpToAgent,
            Produced,
            steer(json!({"steering": {"idleBehavior": "queue"}})),
        ),
        (
            AcpToAgent,
            Consumed,
            acp_result(3, json!({"outcome": "queued"})),
        ),
    ]);
    assert_eq!(mismatch_paths(&wrong[2]), vec!["/params/_meta"]);
    assert_eq!(mismatch_paths(&wrong[3]), vec!["/result"]);
}

#[test]
fn cursor_extensions_have_no_authoritative_definition() {
    let cursor = check_all_on(
        Some(AcpPeer::Cursor),
        &[(AcpToAgent,
        Produced,
        json!({"jsonrpc": "2.0", "id": 4, "method": "cursor/list_available_models", "params": {}}))],
    )
    .remove(0);
    assert_eq!(
        cursor,
        EdgeConformance::NoAuthoritativeDefinition {
            owner: ContractOwner::CursorExtra,
            method: "cursor/list_available_models".to_owned()
        }
    );
}

#[test]
fn an_unclaimed_extension_method_is_incomplete_never_a_pass() {
    let result = check_all_on(
        Some(AcpPeer::Claude),
        &[(
            AcpToAgent,
            Produced,
            json!({"jsonrpc": "2.0", "id": 4, "method": "_unknown/thing", "params": {}}),
        )],
    )
    .remove(0);
    assert!(
        matches!(
            result,
            EdgeConformance::Incomplete {
                missing: MissingContext::UnknownMethod { .. },
                ..
            }
        ),
        "{result:?}"
    );
}

fn copy_of_pins() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("temp dir");
    for entry in std::fs::read_dir(pinned_schema_directory()).expect("read pins") {
        let entry = entry.expect("entry");
        std::fs::copy(entry.path(), directory.path().join(entry.file_name())).expect("copy pin");
    }
    directory
}

#[test]
fn a_pinned_file_with_a_changed_hash_refuses_to_load() {
    let directory = copy_of_pins();
    let file = directory.path().join("acp-schema-1.9.1.json");
    let mut bytes = std::fs::read(&file).expect("read");
    bytes.push(b' ');
    std::fs::write(&file, bytes).expect("write");
    assert!(matches!(
        EdgeDefinitions::load_from(directory.path()),
        Err(PinError::HashChanged { file, .. }) if file == "acp-schema-1.9.1.json"
    ));
}

#[test]
fn a_missing_pinned_file_refuses_to_load() {
    let directory = copy_of_pins();
    std::fs::remove_file(
        directory
            .path()
            .join("codex-app-server-0.166.0.pairing.json"),
    )
    .expect("remove");
    assert!(matches!(
        EdgeDefinitions::load_from(directory.path()),
        Err(PinError::Missing { file, .. }) if file == "codex-app-server-0.166.0.pairing.json"
    ));
}

/// The checks beyond one frame against one definition: Router-owned codecs and verdicts, named
/// bodies, negotiated support, the connection ledger, MCP advertisements and ACP peer ownership.
#[path = "edge_conformance_owner_test.rs"]
mod owner;

/// Correction 3: required bodies, owned `_meta` on every frame, MCP schema meaning and tool
/// identity, and reused request ids.
#[path = "edge_conformance_completeness_test.rs"]
mod completeness;
