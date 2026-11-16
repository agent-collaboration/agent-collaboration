//! OB2, continued: owners beyond the frame — Router-owned codecs and verdicts, named bodies,
//! negotiated support, the connection ledger, MCP advertisements and ACP peer ownership.
use super::*;

/// The owning codec of a test contract: accepts `{"inputs": [...]}` and re-encodes it.
fn accepting_codec(raw: &Value) -> Result<Value, String> {
    let inputs = raw.get("inputs").ok_or("missing inputs")?;
    Ok(json!({"inputs": inputs}))
}

/// A codec that accepts the value but encodes it differently (drops `extra`).
fn lossy_codec(raw: &Value) -> Result<Value, String> {
    Ok(json!({"inputs": raw["inputs"]}))
}

fn queue_list_exchange(codecs: &OwnedCodecs, result: Value) -> EdgeConformance {
    let mut ledger = ConnectionLedger::default();
    let family = MessageFamily::AcpServedForRouterSessions;
    let wire = RecordLayer::WireFrame;
    check_with(
        &mut ledger,
        codecs,
        "case",
        &wire,
        family,
        Consumed,
        &json!({"jsonrpc": "2.0", "id": 5, "method": "_session/queue/list", "params": {}}),
    );
    check_with(
        &mut ledger,
        codecs,
        "case",
        &wire,
        family,
        Produced,
        &json!({"jsonrpc": "2.0", "id": 5, "result": result}),
    )
}

#[test]
fn router_owned_extensions_are_held_to_their_codec() {
    let mut codecs = OwnedCodecs::default();
    codecs.register(RouterContract::QueueListResult, accepting_codec);
    assert!(matches!(
        queue_list_exchange(&codecs, json!({"inputs": []})),
        EdgeConformance::Conforms {
            owner: ContractOwner::RouterExtension,
            ..
        }
    ));
    let rejected = queue_list_exchange(&codecs, json!({"queued": []}));
    assert!(
        matches!(&rejected, EdgeConformance::Violations { violations, .. }
            if violations[0].keyword == "routerCodec"),
        "{rejected:?}"
    );
    let mut lossy = OwnedCodecs::default();
    lossy.register(RouterContract::QueueListResult, lossy_codec);
    assert!(matches!(
        queue_list_exchange(&lossy, json!({"inputs": [], "extra": 1})),
        EdgeConformance::Violations { .. }
    ));
    assert!(matches!(
        queue_list_exchange(&OwnedCodecs::default(), json!({"inputs": []})),
        EdgeConformance::Incomplete {
            missing: MissingContext::NoOwnerCodec { .. },
            ..
        }
    ));
}

#[test]
fn a_router_meta_body_is_held_to_its_codec() {
    let mut codecs = OwnedCodecs::default();
    codecs.register(RouterContract::ModelReasoningMeta, accepting_codec);
    let layer = RecordLayer::Body {
        body: BodyRoot::RouterMeta {
            contract: RouterContract::ModelReasoningMeta,
        },
    };
    let result = check_with(
        &mut ConnectionLedger::default(),
        &codecs,
        "case",
        &layer,
        MessageFamily::ClaudeModelMetadata,
        Consumed,
        &json!({"inputs": ["x"]}),
    );
    assert!(
        matches!(result, EdgeConformance::Conforms { .. }),
        "{result:?}"
    );
}

#[test]
fn an_acp_params_body_is_checked_against_its_request() {
    let layer = RecordLayer::Body {
        body: BodyRoot::AcpRequestParams {
            method: "session/prompt".to_owned(),
        },
    };
    let mut missing = valid_prompt_params();
    missing.as_object_mut().expect("object").remove("sessionId");
    let result = check_with(
        &mut ConnectionLedger::default(),
        &OwnedCodecs::default(),
        "case",
        &layer,
        MessageFamily::AcpRequestToCodexAdapter,
        Consumed,
        &missing,
    );
    assert_eq!(mismatch_paths(&result), vec![""]);
}

fn initialize_exchange(ledger: &mut ConnectionLedger, connection: &str, capabilities: Value) {
    let codecs = OwnedCodecs::default();
    let wire = RecordLayer::WireFrame;
    check_with(
        ledger,
        &codecs,
        connection,
        &wire,
        AcpToAgent,
        Produced,
        &json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": 1}}),
    );
    let result = check_with(
        ledger,
        &codecs,
        connection,
        &wire,
        AcpToAgent,
        Consumed,
        &json!({"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": 1, "agentCapabilities": capabilities}}),
    );
    assert_conforms(&result);
}

fn image_prompt(ledger: &mut ConnectionLedger, connection: &str) -> EdgeConformance {
    let mut params = valid_prompt_params();
    params["prompt"] = json!([{"type": "image", "mimeType": "image/png", "data": "AA=="}]);
    check_with(
        ledger,
        &OwnedCodecs::default(),
        connection,
        &RecordLayer::WireFrame,
        AcpToAgent,
        Produced,
        &acp_prompt(params),
    )
}

#[test]
fn support_is_reported_from_the_negotiated_capabilities() {
    let mut ledger = ConnectionLedger::default();
    initialize_exchange(
        &mut ledger,
        "no-images",
        json!({"promptCapabilities": {"image": false}}),
    );
    initialize_exchange(
        &mut ledger,
        "images",
        json!({"promptCapabilities": {"image": true}}),
    );
    assert!(matches!(
        image_prompt(&mut ledger, "no-images"),
        EdgeConformance::Conforms {
            support: Support::NotNegotiated { .. },
            ..
        }
    ));
    assert!(matches!(
        image_prompt(&mut ledger, "images"),
        EdgeConformance::Conforms {
            support: Support::Negotiated,
            ..
        }
    ));
    assert!(matches!(
        image_prompt(&mut ledger, "never-initialized"),
        EdgeConformance::Incomplete {
            missing: MissingContext::NoNegotiation { .. },
            ..
        }
    ));
}

#[test]
fn equal_ids_on_two_connections_or_directions_never_cross() {
    let mut ledger = ConnectionLedger::default();
    let codecs = OwnedCodecs::default();
    let wire = RecordLayer::WireFrame;
    check_with(
        &mut ledger,
        &codecs,
        "one",
        &wire,
        CodexClientRequestFromCli,
        Consumed,
        &turn_start(valid_turn_start_params()),
    );
    // The same id answered on another connection: an orphan there.
    let other = check_with(
        &mut ledger,
        &codecs,
        "two",
        &wire,
        CodexMessageToCli,
        Produced,
        &turn_result("inProgress"),
    );
    assert!(matches!(
        other,
        EdgeConformance::Incomplete {
            missing: MissingContext::OrphanReply { .. },
            ..
        }
    ));
    // The same id sent by the server is a different request: the client's reply to it is not
    // the answer to the client's own request.
    let reverse = check_with(
        &mut ledger,
        &codecs,
        "one",
        &wire,
        CodexClientRequestFromCli,
        Consumed,
        &turn_result("inProgress"),
    );
    assert!(matches!(
        reverse,
        EdgeConformance::Incomplete {
            missing: MissingContext::OrphanReply { .. },
            ..
        }
    ));
    assert_eq!(ledger.unanswered().len(), 1);
}

fn mcp_initialize() -> Value {
    json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}})
}

/// A tools/list result advertising `tool`, whose input schema embeds the Codex part `native`.
fn tools_list(native: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "result": {"tools": [{
        "name": "set_status",
        "inputSchema": {"type": "object", "required": ["status"],
            "properties": {"status": {"$ref": "#/$defs/codexNativeTurnStatus"}},
            "$defs": {"codexNativeTurnStatus": native}},
        "outputSchema": {"type": "object", "required": ["ok"], "properties": {"ok": {"type": "boolean"}}}
    }]}})
}

fn mcp_exchange(native: Value, arguments: Value, structured: Value) -> Vec<EdgeConformance> {
    let call = MessageFamily::McpToolCall;
    check_all(&[
        (call, Consumed, mcp_initialize()),
        (
            call,
            Produced,
            json!({"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": "2025-06-18",
            "capabilities": {"tools": {}}, "serverInfo": {"name": "router", "version": "1"}}}),
        ),
        (
            call,
            Consumed,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
        ),
        (call, Produced, tools_list(native)),
        (
            call,
            Consumed,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "set_status", "arguments": arguments}}),
        ),
        (
            call,
            Produced,
            json!({"jsonrpc": "2.0", "id": 2, "result": {"content": [], "structuredContent": structured}}),
        ),
    ])
}

fn pinned_turn_status() -> Value {
    json!({"enum": ["completed", "interrupted", "failed", "inProgress"], "type": "string"})
}

#[test]
fn mcp_valid_exchange_conforms() {
    mcp_exchange(
        pinned_turn_status(),
        json!({"status": "completed"}),
        json!({"ok": true}),
    )
    .iter()
    .for_each(assert_conforms);
}

#[test]
fn mcp_arguments_and_structured_content_are_held_to_the_advertised_schemas() {
    let results = mcp_exchange(
        pinned_turn_status(),
        json!({"status": "running"}),
        json!({"ok": "yes"}),
    );
    assert_eq!(
        mismatch_paths(&results[4]),
        vec!["/params/arguments/status"]
    );
    assert_eq!(
        mismatch_paths(&results[5]),
        vec!["/result/structuredContent/ok"]
    );
}

#[test]
fn an_mcp_codex_part_that_differs_from_the_pin_is_a_violation() {
    // `{}` is what schema_binding.rs writes when no native bundle is available.
    let results = mcp_exchange(
        json!({}),
        json!({"status": "completed"}),
        json!({"ok": true}),
    );
    match &results[3] {
        EdgeConformance::Violations { violations, .. } => {
            assert!(
                violations
                    .iter()
                    .any(|violation| violation.keyword == "codexParts"),
                "{violations:?}"
            );
        }
        other => panic!("expected violations, got {other:?}"),
    }
}

#[test]
fn an_mcp_call_to_an_unadvertised_tool_is_incomplete_and_still_envelope_checked() {
    let call = MessageFamily::McpToolCall;
    let results = check_all(&[
        (
            call,
            Consumed,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "set_status", "arguments": {}}}),
        ),
        (
            call,
            Consumed,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"arguments": {}}}),
        ),
    ]);
    let leaves = |result: &EdgeConformance| result.leaves().cloned().collect::<Vec<_>>();
    assert!(
        leaves(&results[0]).iter().any(|leaf| matches!(
            leaf,
            EdgeConformance::Incomplete {
                missing: MissingContext::NoAdvertisement { .. },
                ..
            }
        )),
        "{:?}",
        results[0]
    );
    // A malformed call (no tool name) on a connection with no advertisement still gets its
    // envelope decode: the missing advertisement does not hide the rmcp violation.
    assert!(
        leaves(&results[1]).iter().any(|leaf| matches!(
            leaf,
            EdgeConformance::Violations { violations, .. }
                if violations.iter().any(|violation| violation.keyword == "rmcpDecode")
        )),
        "{:?}",
        results[1]
    );
}

#[test]
fn an_mcp_frame_rmcp_cannot_decode_is_a_violation() {
    let result = check_one(
        MessageFamily::McpToolCall,
        Consumed,
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"capabilities": {}}}),
    );
    match result {
        EdgeConformance::Violations { violations, .. } => {
            assert_eq!(violations[0].keyword, "rmcpDecode")
        }
        other => panic!("expected violations, got {other:?}"),
    }
}

#[test]
fn typed_records_are_never_schema_checked() {
    let result = check_with(
        &mut ConnectionLedger::default(),
        &OwnedCodecs::default(),
        "case",
        &RecordLayer::TypedInput,
        AcpToAgent,
        Consumed,
        &json!({"anything": true}),
    );
    assert_eq!(
        result,
        EdgeConformance::NotSchemaChecked {
            layer: RecordLayer::TypedInput
        }
    );
}

fn agreeing_verdicts(raw: &Value) -> Result<(Value, Value), String> {
    Ok((json!({"accepted": raw}), json!({"accepted": raw})))
}

/// Zod strips an unknown key the Rust decoder rejects: a disagreement.
fn disagreeing_verdicts(raw: &Value) -> Result<(Value, Value), String> {
    Ok((
        json!({"accepted": raw}),
        json!({"rejected": "unknown field"}),
    ))
}

#[test]
fn model_metadata_is_held_to_its_owners_agreeing() {
    let layer = RecordLayer::Body {
        body: BodyRoot::RouterMeta {
            contract: RouterContract::ModelReasoningMeta,
        },
    };
    let check = |codecs: &OwnedCodecs| {
        check_with(
            &mut ConnectionLedger::default(),
            codecs,
            "case",
            &layer,
            MessageFamily::ClaudeModelMetadata,
            Consumed,
            &json!({"version": 1}),
        )
    };
    let mut agree = OwnedCodecs::default();
    agree.register_verdicts(RouterContract::ModelReasoningMeta, agreeing_verdicts);
    assert!(matches!(check(&agree), EdgeConformance::Conforms { .. }));
    let mut disagree = OwnedCodecs::default();
    disagree.register_verdicts(RouterContract::ModelReasoningMeta, disagreeing_verdicts);
    assert!(
        matches!(check(&disagree), EdgeConformance::Violations { ref violations, .. }
            if violations[0].detail == "zodVsRust"),
    );
}

#[test]
fn a_caller_of_the_served_n8_edge_is_its_client() {
    let family = MessageFamily::AcpRequestToCodexAdapter;
    assert_eq!(
        family_edge(family, ProtocolNode::N8CodexAcpAdapter),
        Some((crate::node_taps::OutsideEdge::AcpServedN8, true))
    );
    assert_eq!(
        family_edge(family, ProtocolNode::N11McpServer),
        Some((crate::node_taps::OutsideEdge::AcpServedN8, false))
    );
}

fn auth_status_update(kind: &str) -> Value {
    json!({"jsonrpc": "2.0", "method": "_auth/status_update",
        "params": {"authStatus": {"kind": kind, "label": "Claude Max"}}})
}

#[test]
fn claude_auth_status_is_checked_against_the_claude_pin_on_a_claude_connection() {
    let claude = check_all_on(
        Some(AcpPeer::Claude),
        &[(AcpToAgent, Consumed, auth_status_update("account"))],
    );
    assert!(
        matches!(
            &claude[0],
            EdgeConformance::Conforms {
                owner: ContractOwner::ClaudeExtra,
                ..
            }
        ),
        "{claude:?}"
    );
    let wrong = check_all_on(
        Some(AcpPeer::Claude),
        &[(AcpToAgent, Consumed, auth_status_update("password"))],
    );
    assert_eq!(mismatch_paths(&wrong[0]), vec!["/params/authStatus/kind"]);
}

#[test]
fn the_same_method_on_a_cursor_connection_has_no_authoritative_definition() {
    let cursor = check_all_on(
        Some(AcpPeer::Cursor),
        &[(AcpToAgent, Consumed, auth_status_update("account"))],
    );
    assert_eq!(
        cursor[0],
        EdgeConformance::NoAuthoritativeDefinition {
            owner: ContractOwner::CursorExtra,
            method: "_auth/status_update".to_owned()
        }
    );
}

#[test]
fn an_extension_method_with_no_known_agent_is_incomplete() {
    let unknown = check_all_on(
        None,
        &[(AcpToAgent, Consumed, auth_status_update("account"))],
    );
    assert!(
        matches!(
            &unknown[0],
            EdgeConformance::Incomplete {
                missing: MissingContext::UnknownPeer { .. },
                ..
            }
        ),
        "{unknown:?}"
    );
}

#[test]
fn the_negotiated_claude_package_name_makes_the_connection_claudes() {
    let [init, init_result] = steering_initialize();
    let results = check_all(&[
        init,
        init_result,
        (AcpToAgent, Consumed, auth_status_update("account")),
    ]);
    assert!(
        matches!(
            &results[2],
            EdgeConformance::Conforms {
                owner: ContractOwner::ClaudeExtra,
                ..
            }
        ),
        "{results:?}"
    );
}

/// The owner check could not run (node unavailable, a nonzero exit, unreadable output).
fn unavailable_verdicts(_: &Value) -> Result<(Value, Value), String> {
    Err("run node on the Zod verdict: No such file or directory".to_owned())
}

#[test]
fn an_owner_check_that_cannot_run_is_incomplete_not_a_violation() {
    let mut codecs = OwnedCodecs::default();
    codecs.register_verdicts(RouterContract::ModelReasoningMeta, unavailable_verdicts);
    let layer = RecordLayer::Body {
        body: BodyRoot::RouterMeta {
            contract: RouterContract::ModelReasoningMeta,
        },
    };

    let result = check_with(
        &mut ConnectionLedger::default(),
        &codecs,
        "case",
        &layer,
        MessageFamily::ClaudeModelMetadata,
        Produced,
        &json!({"version": 1, "kind": "unknown", "reason": "Unavailable"}),
    );

    assert!(
        matches!(
            &result,
            EdgeConformance::Incomplete {
                missing: MissingContext::OwnerOutcomeUnavailable {
                    contract: RouterContract::ModelReasoningMeta,
                    ..
                },
                ..
            }
        ),
        "{result:?}"
    );
}
