//! OB2, correction 3: nothing a frame carries escapes its owner. A required body is required;
//! owned `_meta` is held to its owner on every frame, with each component's verdict kept; MCP
//! schemas compare by meaning and keep each tool's identity; a reused request id is reported.
use super::*;
use crate::node_taps::edge_conformance::Violation;

use MessageFamily::{
    AcpRequestToCodexAdapter, AcpServedForCodexSessions, AcpServedForRouterSessions, McpToolCall,
};

/// Every violation of every component of `result`.
fn violations(result: &EdgeConformance) -> Vec<Violation> {
    result
        .leaves()
        .filter_map(|leaf| match leaf {
            EdgeConformance::Violations { violations, .. } => Some(violations.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Every missing check of every component of `result`.
fn missing(result: &EdgeConformance) -> Vec<MissingContext> {
    result
        .leaves()
        .filter_map(|leaf| match leaf {
            EdgeConformance::Incomplete { missing, .. } => Some(missing.clone()),
            _ => None,
        })
        .collect()
}

fn has_required(result: &EdgeConformance, property: &str) -> bool {
    violations(result)
        .iter()
        .any(|violation| violation.keyword == "required" && violation.detail == property)
}

/// Checks frames on one connection with `codecs` registered.
fn check_frames(
    codecs: &OwnedCodecs,
    peer: Option<AcpPeer>,
    frames: &[(MessageFamily, TapDirection, Value)],
) -> Vec<EdgeConformance> {
    let mut ledger = ConnectionLedger::default();
    if let Some(peer) = peer {
        ledger.declare_peer(&ConnectionId("case".to_owned()), peer);
    }
    frames
        .iter()
        .map(|(family, direction, raw)| {
            check_with(
                &mut ledger,
                codecs,
                "case",
                &RecordLayer::WireFrame,
                *family,
                *direction,
                raw,
            )
        })
        .collect()
}

// A: a method whose contract requires params must carry them.

#[test]
fn an_acp_request_or_notification_without_its_required_body_is_a_violation() {
    let prompt = check_one(
        AcpToAgent,
        Produced,
        json!({"jsonrpc": "2.0", "id": 1, "method": "session/prompt"}),
    );
    let update = check_one(
        AcpToAgent,
        Consumed,
        json!({"jsonrpc": "2.0", "method": "session/update"}),
    );

    assert!(has_required(&prompt, "params"), "{prompt:?}");
    assert!(has_required(&update, "params"), "{update:?}");
}

#[test]
fn a_claude_extension_request_without_its_required_body_is_a_violation() {
    let [init, init_result] = steering_initialize();
    let results = check_all(&[
        init,
        init_result,
        (
            AcpToAgent,
            Produced,
            json!({"jsonrpc": "2.0", "id": 3, "method": "_session/steering"}),
        ),
    ]);

    assert!(has_required(&results[2], "params"), "{:?}", results[2]);
}

// B: owned `_meta` on every captured frame.

/// A stand-in for N9's session result `_meta` owner, for dispatch tests only: it rejects a
/// `sessionProfile` key other than `capabilities`, as `SessionResponseProfileMetadata` does.
fn session_result_codec(raw: &Value) -> Result<Value, String> {
    let profile = raw
        .get("sessionProfile")
        .and_then(Value::as_object)
        .ok_or("missing sessionProfile")?;
    if let Some(unknown) = profile.keys().find(|key| key.as_str() != "capabilities") {
        return Err(format!("unknown field `{unknown}`"));
    }
    Ok(raw.clone())
}

fn rejecting_codec(_: &Value) -> Result<Value, String> {
    Err("rejected".to_owned())
}

fn session_new_exchange(codecs: &OwnedCodecs, meta: Value) -> EdgeConformance {
    check_frames(
        codecs,
        None,
        &[
            (
                AcpServedForRouterSessions,
                Consumed,
                json!({"jsonrpc": "2.0", "id": 1, "method": "session/new",
                    "params": {"cwd": "/work", "mcpServers": []}}),
            ),
            (
                AcpServedForRouterSessions,
                Produced,
                acp_result(1, json!({"sessionId": "session-1", "_meta": meta})),
            ),
        ],
    )
    .remove(1)
}

#[test]
fn n9_session_result_meta_dispatches_to_its_owning_codec() {
    // Dispatch and merge only: a stand-in codec, not the real owner. The real owner's accepted and
    // rejected values are controlled against the real registry in acp-provider-adapters
    // (tests/integration/owned_contract_checks.rs).
    let mut codecs = OwnedCodecs::default();
    codecs.register(RouterContract::SessionResultMeta, session_result_codec);

    let rejected = session_new_exchange(
        &codecs,
        json!({"sessionProfile": {"capabilities": {}, "historyUnavailable": false}}),
    );
    let unregistered = session_new_exchange(
        &OwnedCodecs::default(),
        json!({"sessionProfile": {"capabilities": {}}}),
    );

    assert!(
        violations(&rejected)
            .iter()
            .any(|violation| violation.keyword == "routerCodec"
                && violation.instance_path == "/result/_meta"),
        "{rejected:?}"
    );
    assert!(
        missing(&unregistered).contains(&MissingContext::NoOwnerCodec {
            contract: RouterContract::SessionResultMeta
        }),
        "{unregistered:?}"
    );
}

#[test]
fn a_router_owned_meta_key_with_no_owner_check_is_reported_not_passed() {
    let results = check_frames(
        &OwnedCodecs::default(),
        None,
        &[
            (
                AcpRequestToCodexAdapter,
                Consumed,
                acp_prompt(valid_prompt_params()),
            ),
            (
                AcpServedForCodexSessions,
                Produced,
                acp_result(
                    1,
                    json!({"stopReason": "end_turn",
                        "_meta": {"codex-router/finalReply": {"kind": "available", "text": null}}}),
                ),
            ),
        ],
    );

    assert!(
        missing(&results[1]).contains(&MissingContext::MissingOwner {
            pointer: "/result/_meta/codex-router~1finalReply".to_owned()
        }),
        "{:?}",
        results[1]
    );
}

#[test]
fn an_acp_violation_does_not_hide_the_owned_meta_violation_of_the_same_frame() {
    let mut codecs = OwnedCodecs::default();
    codecs.register(RouterContract::InitializeMeta, rejecting_codec);

    let results = check_frames(
        &codecs,
        None,
        &[
            (
                AcpServedForRouterSessions,
                Consumed,
                json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": 1}}),
            ),
            (
                AcpServedForRouterSessions,
                Produced,
                acp_result(
                    0,
                    json!({"protocolVersion": "one", "_meta": {"sessionProfile": {}}}),
                ),
            ),
        ],
    );
    let found = violations(&results[1]);

    assert!(
        found
            .iter()
            .any(|violation| violation.instance_path == "/result/protocolVersion"),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|violation| violation.keyword == "routerCodec"
                && violation.instance_path == "/result/_meta"),
        "{found:?}"
    );
}

#[test]
fn claude_initialize_meta_is_held_to_the_pinned_x_meta() {
    let initialize = |meta: Value| {
        check_all(&[
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
                        "_meta": meta}),
                ),
            ),
        ])
        .remove(1)
    };

    let advertised = initialize(json!({
        "steering": {"supported": true},
        "goal": {"version": 1, "controlMethod": "_session/goal", "actions": ["set", "clear"]},
        "jetbrains": {"air": {"version": 1, "capabilities": ["sessionFailure"]}}}));
    let wrong_steering = initialize(json!({"steering": {"supported": "yes"}}));
    let wrong_goal = initialize(
        json!({"goal": {"version": 1, "controlMethod": "_session/other",
        "actions": ["set"]}}),
    );

    assert!(violations(&advertised).is_empty(), "{advertised:?}");
    assert!(
        violations(&wrong_steering)
            .iter()
            .any(|violation| violation.instance_path == "/result/_meta/steering/supported"),
        "{wrong_steering:?}"
    );
    assert!(
        violations(&wrong_goal)
            .iter()
            .any(|violation| violation.instance_path == "/result/_meta/goal/controlMethod"),
        "{wrong_goal:?}"
    );
}

#[test]
fn skill_annotations_are_held_to_their_owner_at_their_own_place() {
    let [init, init_result] = steering_initialize();
    let update = json!({"jsonrpc": "2.0", "method": "session/update", "params": {
    "sessionId": "session-1",
    "update": {
        "sessionUpdate": "available_commands_update",
        "availableCommands": [
            {"name": "plain", "description": "no annotation"},
            {"name": "review", "description": "a skill",
                "_meta": {"codexRouter": {"kind": "skill"}}}
        ],
        "_meta": {"codexRouter": {"skillStatus": "available"}}
    }}});
    let mut codecs = OwnedCodecs::default();
    codecs.register(RouterContract::SkillListMeta, |raw| Ok(raw.clone()));
    codecs.register(RouterContract::SkillCommandMeta, rejecting_codec);

    let unregistered = check_all(&[
        init.clone(),
        init_result.clone(),
        (AcpToAgent, Consumed, update.clone()),
    ])
    .remove(2);
    let registered = check_frames(
        &codecs,
        None,
        &[init, init_result, (AcpToAgent, Consumed, update)],
    )
    .remove(2);

    let unchecked = missing(&unregistered);
    assert!(
        unchecked.contains(&MissingContext::NoOwnerCodec {
            contract: RouterContract::SkillListMeta
        }) && unchecked.contains(&MissingContext::NoOwnerCodec {
            contract: RouterContract::SkillCommandMeta
        }),
        "{unregistered:?}"
    );
    let found = violations(&registered);
    assert_eq!(
        found
            .iter()
            .map(|violation| violation.instance_path.as_str())
            .collect::<Vec<_>>(),
        vec!["/params/update/availableCommands/1/_meta/codexRouter"],
        "{registered:?}"
    );
}

// F: `_session/goal` is pinned.

#[test]
fn claude_goal_control_is_checked_against_the_pin() {
    let goal = |params: Value| {
        let [init, init_result] = steering_initialize();
        check_all(&[
            init,
            init_result,
            (
                AcpToAgent,
                Produced,
                json!({"jsonrpc": "2.0", "id": 4, "method": "_session/goal", "params": params}),
            ),
            (AcpToAgent, Consumed, acp_result(4, json!({}))),
        ])
    };

    let set = goal(json!({"sessionId": "session-1", "action": "set", "objective": "ship"}));
    let incomplete_set = goal(json!({"sessionId": "session-1", "action": "set"}));

    assert!(
        set[2..].iter().all(|result| violations(result).is_empty()),
        "{set:?}"
    );
    assert!(
        !violations(&incomplete_set[2]).is_empty(),
        "{incomplete_set:?}"
    );
}

// C and D: MCP Codex parts compare by meaning, and keep each tool's place.

/// A pinned Codex definition written the way a different serializer spells it: integral numbers
/// as integers, and boolean schemas as `{}` and `{"not":{}}`.
fn respelled(value: &Value) -> Value {
    fn schema(value: &Value) -> Value {
        match value {
            Value::Bool(true) => json!({}),
            Value::Bool(false) => json!({"not": {}}),
            other => respelled(other),
        }
    }
    match value {
        Value::Number(number) => match number.as_f64() {
            Some(float) if number.is_f64() && float.fract() == 0.0 => {
                #[expect(clippy::cast_possible_truncation, reason = "integral test values")]
                let integer = float as i64;
                json!(integer)
            }
            _ => value.clone(),
        },
        Value::Array(items) => Value::Array(items.iter().map(respelled).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, child)| {
                    let rewritten = match key.as_str() {
                        "additionalProperties" | "items" => schema(child),
                        "properties" => match child {
                            Value::Object(properties) => Value::Object(
                                properties
                                    .iter()
                                    .map(|(name, property)| (name.clone(), schema(property)))
                                    .collect(),
                            ),
                            other => respelled(other),
                        },
                        _ => respelled(child),
                    };
                    (key.clone(), rewritten)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn pinned(name: &str) -> Value {
    definitions().codex_v2_definitions()[name].clone()
}

/// A tools/list exchange whose tools each embed `defs` as Codex parts.
fn tools_list_with(tools: &[(&str, Value)]) -> EdgeConformance {
    let listed: Vec<Value> = tools
        .iter()
        .map(|(name, defs)| json!({"name": name, "inputSchema": {"type": "object", "$defs": defs}}))
        .collect();
    check_frames(
        &OwnedCodecs::default(),
        None,
        &[
            (
                McpToolCall,
                Consumed,
                json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
            ),
            (
                McpToolCall,
                Produced,
                json!({"jsonrpc": "2.0", "id": 1, "result": {"tools": listed}}),
            ),
        ],
    )
    .remove(1)
}

fn codex_parts(result: &EdgeConformance) -> Vec<String> {
    violations(result)
        .into_iter()
        .filter(|violation| violation.keyword == "codexParts")
        .map(|violation| violation.instance_path)
        .collect()
}

#[test]
fn an_equivalent_spelling_of_a_codex_part_is_not_a_violation() {
    let defs = json!({
        "codexNativeByteRange": respelled(&pinned("ByteRange")),
        "codexNativeAnalyticsConfig": respelled(&pinned("AnalyticsConfig")),
        "codexNativeMcpToolCallResult": respelled(&pinned("McpToolCallResult")),
        "codexNativeUserVerificationDeleteParams":
            respelled(&pinned("UserVerificationDeleteParams")),
    });
    assert_ne!(defs["codexNativeByteRange"], pinned("ByteRange"));

    let result = tools_list_with(&[("session_inspect", defs)]);

    assert!(codex_parts(&result).is_empty(), "{result:?}");
}

#[test]
fn a_real_difference_in_a_codex_part_is_still_a_violation() {
    let mut changed = respelled(&pinned("ByteRange"));
    changed["properties"]["start"]["minimum"] = json!(1);

    let result = tools_list_with(&[("session_inspect", json!({"codexNativeByteRange": changed}))]);

    assert_eq!(
        codex_parts(&result),
        vec!["/result/tools/0/inputSchema/$defs/codexNativeByteRange"]
    );
}

#[test]
fn the_same_codex_part_defect_in_two_tools_is_two_defects() {
    let wrong = json!({"codexNativeTurnStatus": {}});

    let both = tools_list_with(&[("tool_a", wrong.clone()), ("tool_b", wrong.clone())]);
    let only_b = tools_list_with(&[
        (
            "tool_a",
            json!({"codexNativeTurnStatus": pinned("TurnStatus")}),
        ),
        ("tool_b", wrong),
    ]);

    assert_eq!(
        codex_parts(&both),
        vec![
            "/result/tools/0/inputSchema/$defs/codexNativeTurnStatus",
            "/result/tools/1/inputSchema/$defs/codexNativeTurnStatus",
        ]
    );
    // Fixing tool A removes exactly its identity.
    assert_eq!(
        codex_parts(&only_b),
        vec!["/result/tools/1/inputSchema/$defs/codexNativeTurnStatus"]
    );
}

// H: a reused request id is reported and the first pairing kept.

#[test]
fn a_request_reusing_an_outstanding_id_is_a_violation_and_the_first_pairing_holds() {
    let results = check_all(&[
        (AcpToAgent, Produced, acp_prompt(valid_prompt_params())),
        (
            AcpToAgent,
            Produced,
            json!({"jsonrpc": "2.0", "id": 1, "method": "session/set_mode",
                "params": {"sessionId": "session-1", "modeId": "ask"}}),
        ),
        (
            AcpToAgent,
            Consumed,
            acp_result(1, json!({"stopReason": "end_turn"})),
        ),
    ]);

    assert!(
        violations(&results[1])
            .iter()
            .any(|violation| violation.keyword == "duplicateId"),
        "{:?}",
        results[1]
    );
    assert!(
        matches!(&results[2], EdgeConformance::Conforms { definition, .. }
            if definition.ends_with(":session/prompt")),
        "{:?}",
        results[2]
    );
}

#[test]
fn model_reasoning_on_a_config_option_update_is_held_to_its_owner() {
    let [init, init_result] = steering_initialize();
    let update = json!({"jsonrpc": "2.0", "method": "session/update", "params": {
    "sessionId": "session-1",
    "update": {
        "sessionUpdate": "config_option_update",
        "configOptions": [],
        "_meta": {"codexRouter": {"modelReasoning": {"version": 1, "kind": "unknown"}}}
    }}});

    let result = check_all(&[init, init_result, (AcpToAgent, Consumed, update)]).remove(2);

    assert!(
        missing(&result).contains(&MissingContext::NoOwnerCodec {
            contract: RouterContract::ModelReasoningMeta
        }),
        "{result:?}"
    );
}

// Correction 4.

fn claude_initialize_with(meta: Value) -> EdgeConformance {
    check_all(&[
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
                    "_meta": meta}),
            ),
        ),
    ])
    .remove(1)
}

#[test]
fn an_open_claude_meta_owner_does_not_cover_a_router_key_beside_it() {
    let initialize = claude_initialize_with(json!({
        "steering": {"supported": true},
        "codexRouter": {"uncheckedOwnedKey": 7}}));
    let [init, init_result] = steering_initialize();
    let info = check_all(&[
        init,
        init_result,
        (
            AcpToAgent,
            Consumed,
            json!({"jsonrpc": "2.0", "method": "session/update", "params": {
                "sessionId": "session-1",
                "update": {"sessionUpdate": "session_info_update",
                    "_meta": {"goal": null, "router": {"origin": 1}}}}}),
        ),
    ])
    .remove(2);

    assert!(
        missing(&initialize).contains(&MissingContext::MissingOwner {
            pointer: "/result/_meta/codexRouter".to_owned()
        }),
        "{initialize:?}"
    );
    assert!(
        missing(&info).contains(&MissingContext::MissingOwner {
            pointer: "/params/update/_meta/router".to_owned()
        }),
        "{info:?}"
    );
}

#[test]
fn a_malformed_or_orphan_frame_still_reports_its_router_keys() {
    let malformed = check_one(
        AcpToAgent,
        Consumed,
        json!({"jsonrpc": "2.0", "_meta": {"router": {"sessionRef": 1}}}),
    );
    let orphan = check_one(
        AcpToAgent,
        Consumed,
        acp_result(42, json!({"_meta": {"router": {}}})),
    );

    for result in [&malformed, &orphan] {
        assert!(
            missing(result).contains(&MissingContext::MissingOwner {
                pointer: if std::ptr::eq(result, &malformed) {
                    "/_meta/router".to_owned()
                } else {
                    "/result/_meta/router".to_owned()
                }
            }),
            "{result:?}"
        );
    }
}

#[test]
fn a_reply_without_its_body_pairs_and_is_a_required_result_violation() {
    let results = check_all(&[
        (
            AcpServedForRouterSessions,
            Consumed,
            json!({"jsonrpc": "2.0", "id": 1, "method": "session/new",
                "params": {"cwd": "/work", "mcpServers": []}}),
        ),
        (
            AcpServedForRouterSessions,
            Produced,
            json!({"jsonrpc": "2.0", "id": 1}),
        ),
        // The pending entry is settled: a second reply with that id is now an orphan.
        (
            AcpServedForRouterSessions,
            Produced,
            acp_result(1, json!({"sessionId": "session-1"})),
        ),
    ]);

    assert!(has_required(&results[1], "result"), "{:?}", results[1]);
    assert!(
        missing(&results[2]).contains(&MissingContext::OrphanReply { id: json!(1) }),
        "{:?}",
        results[2]
    );
}

#[test]
fn a_missing_negotiation_is_kept_beside_a_standard_violation() {
    // An image prompt (support needs the negotiation) that also breaks its schema.
    let result = check_one(
        AcpToAgent,
        Produced,
        acp_prompt(
            json!({"sessionId": 5, "prompt": [{"type": "image", "data": "AA==", "mimeType": "image/png"}]}),
        ),
    );

    assert!(!violations(&result).is_empty(), "{result:?}");
    assert!(
        missing(&result)
            .iter()
            .any(|missing| matches!(missing, MissingContext::NoNegotiation { .. })),
        "{result:?}"
    );
}

fn goal_request(meta: Value, params: Value) -> EdgeConformance {
    check_all(&[
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
                    "_meta": meta}),
            ),
        ),
        (
            AcpToAgent,
            Produced,
            json!({"jsonrpc": "2.0", "id": 4, "method": "_session/goal", "params": params}),
        ),
    ])
    .remove(2)
}

fn support_of(result: &EdgeConformance) -> Option<Support> {
    result.leaves().find_map(|leaf| match leaf {
        EdgeConformance::Conforms { support, .. } => Some(support.clone()),
        _ => None,
    })
}

#[test]
fn a_goal_request_is_held_to_the_installed_parser_and_its_own_advertisement() {
    let goal = json!({"version": 1, "controlMethod": "_session/goal", "actions": ["set", "clear"]});

    // Each constraint of the installed parser fails on its own (correction 5).
    let empty_session = goal_request(
        json!({"steering": {"supported": true}, "goal": goal}),
        json!({"sessionId": "", "action": "set", "objective": "ship"}),
    );
    let blank_objective = goal_request(
        json!({"steering": {"supported": true}, "goal": goal}),
        json!({"sessionId": "session-1", "action": "set", "objective": "   "}),
    );
    let goal_only = goal_request(
        json!({"goal": goal}),
        json!({"sessionId": "session-1", "action": "clear"}),
    );
    let steering_only = goal_request(
        json!({"steering": {"supported": true}}),
        json!({"sessionId": "session-1", "action": "clear"}),
    );

    assert!(!violations(&empty_session).is_empty(), "{empty_session:?}");
    assert!(
        !violations(&blank_objective).is_empty(),
        "{blank_objective:?}"
    );
    assert_eq!(
        support_of(&goal_only),
        Some(Support::Negotiated),
        "{goal_only:?}"
    );
    assert_eq!(
        support_of(&steering_only),
        Some(Support::NotNegotiated {
            capability: "goal".to_owned()
        }),
        "{steering_only:?}"
    );
}

#[test]
fn a_numeric_id_is_one_id_whatever_its_spelling() {
    let new_session = |id: Value| {
        json!({"jsonrpc": "2.0", "id": id, "method": "session/new",
            "params": {"cwd": "/work", "mcpServers": []}})
    };
    let results = check_all(&[
        (AcpServedForRouterSessions, Consumed, new_session(json!(1))),
        (
            AcpServedForRouterSessions,
            Consumed,
            new_session(json!(1.0)),
        ),
        (
            AcpServedForRouterSessions,
            Produced,
            json!({"jsonrpc": "2.0", "id": 1.0, "result": {"sessionId": "session-1"}}),
        ),
        (
            AcpServedForRouterSessions,
            Consumed,
            new_session(json!("1")),
        ),
    ]);

    assert!(
        violations(&results[1])
            .iter()
            .any(|violation| violation.keyword == "duplicateId"),
        "{:?}",
        results[1]
    );
    assert!(missing(&results[2]).is_empty(), "{:?}", results[2]);
    // A string id is a different id from the number.
    assert!(
        !violations(&results[3])
            .iter()
            .any(|violation| violation.keyword == "duplicateId"),
        "{:?}",
        results[3]
    );
}

#[test]
fn each_partial_fix_of_two_tools_names_the_defect_it_removed_through_settlement() {
    use crate::node_taps::expected_failures::{Deliverable, RecordKey, SchemaDefect};
    use crate::node_taps::schema_settlement::{Observed, RecordIntent, settle};

    const LISTED: [SchemaDefect; 2] = [
        SchemaDefect {
            deliverable: Deliverable::D1,
            record: RecordKey {
                direction: TapDirection::Produced,
                kind: "response:tools/list",
                ordinal: 0,
            },
            schema_path: "#/definitions/v2/TurnStatus",
            instance_path: "/result/tools/0/inputSchema/$defs/codexNativeTurnStatus",
            keyword: "codexParts",
            detail: "",
        },
        SchemaDefect {
            deliverable: Deliverable::D1,
            record: RecordKey {
                direction: TapDirection::Produced,
                kind: "response:tools/list",
                ordinal: 0,
            },
            schema_path: "#/definitions/v2/TurnStatus",
            instance_path: "/result/tools/1/inputSchema/$defs/codexNativeTurnStatus",
            keyword: "codexParts",
            detail: "",
        },
    ];
    let wrong = json!({"codexNativeTurnStatus": {}});
    let right = json!({"codexNativeTurnStatus": pinned("TurnStatus")});
    let settled_against_list = |tools: &[(&str, Value)]| {
        let mut observed = Observed::default();
        let parts: Vec<Violation> = violations(&tools_list_with(tools))
            .into_iter()
            .filter(|violation| violation.keyword == "codexParts")
            .collect();
        observed.record_violations(
            TapDirection::Produced,
            "response:tools/list",
            0,
            &RecordIntent::Product,
            &parts,
        );
        let settlement = settle(observed, &LISTED, Vec::new(), Vec::new());
        settlement
            .fixed
            .into_iter()
            .map(|defect| defect.identity.instance_path)
            .collect::<Vec<_>>()
    };

    let a_fixed = settled_against_list(&[("tool_a", right.clone()), ("tool_b", wrong.clone())]);
    let b_fixed = settled_against_list(&[("tool_a", wrong.clone()), ("tool_b", right.clone())]);
    let both_fixed = settled_against_list(&[("tool_a", right.clone()), ("tool_b", right)]);
    let none_fixed = settled_against_list(&[("tool_a", wrong.clone()), ("tool_b", wrong)]);

    assert_eq!(a_fixed, vec![LISTED[0].instance_path]);
    assert_eq!(b_fixed, vec![LISTED[1].instance_path]);
    assert_eq!(
        both_fixed,
        vec![LISTED[0].instance_path, LISTED[1].instance_path]
    );
    assert!(none_fixed.is_empty());
}

// Correction 5.

#[test]
fn a_router_key_nested_under_an_open_claude_property_is_reported() {
    let result = claude_initialize_with(json!({
        "steering": {"supported": true},
        "goal": {"version": 1, "controlMethod": "_session/goal", "actions": ["set"],
            "_meta": {"router": {"unchecked": true}}}}));

    assert!(
        missing(&result).contains(&MissingContext::MissingOwner {
            pointer: "/result/_meta/goal/_meta/router".to_owned()
        }),
        "{result:?}"
    );
}

#[test]
fn an_acp_reply_with_both_result_and_error_is_a_violation() {
    let results = check_all(&[
        (
            AcpServedForRouterSessions,
            Consumed,
            json!({"jsonrpc": "2.0", "id": 1, "method": "session/new",
                "params": {"cwd": "/work", "mcpServers": []}}),
        ),
        (
            AcpServedForRouterSessions,
            Produced,
            json!({"jsonrpc": "2.0", "id": 1, "result": {"sessionId": "session-1"},
                "error": {"code": -32603, "message": "also failed"}}),
        ),
    ]);

    assert!(
        violations(&results[1])
            .iter()
            .any(|violation| violation.keyword == "oneOf" && violation.detail == "result,error"),
        "{:?}",
        results[1]
    );
}

#[test]
fn the_int64_minimum_and_its_float_spelling_are_one_id() {
    let new_session = |id: Value| {
        json!({"jsonrpc": "2.0", "id": id, "method": "session/new",
            "params": {"cwd": "/work", "mcpServers": []}})
    };
    let minimum: Value =
        serde_json::from_str("-9223372036854775808").unwrap_or_else(|error| panic!("{error}"));
    let minimum_float: Value =
        serde_json::from_str("-9223372036854775808.0").unwrap_or_else(|error| panic!("{error}"));

    let results = check_all(&[
        (AcpServedForRouterSessions, Consumed, new_session(minimum)),
        (
            AcpServedForRouterSessions,
            Consumed,
            new_session(minimum_float),
        ),
    ]);

    assert!(
        violations(&results[1])
            .iter()
            .any(|violation| violation.keyword == "duplicateId"),
        "{:?}",
        results[1]
    );
}
