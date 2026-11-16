//! Correction 4, V2: the real owner registry (`src/owned_contract_checks.rs`) reports each owner's
//! actual outcome on whole captured-shape frames, through the edge checker the part tests and the
//! live recording checker use. The cases are the Verifier's probes, as frame mutations: no stand-in
//! codec decides any of them.

use codex_router_test_support::node_taps::edge_conformance::MissingContext;
use codex_router_test_support::node_taps::{
    AcpPeer, CheckInput, ConnectionId, ConnectionLedger, EdgeConformance, EdgeDefinitions,
    OutsideEdge, OwnedCodecs, ProtocolRole, RecordLayer, Violation,
};
use serde_json::{Value, json};

#[expect(
    clippy::expect_used,
    reason = "test fixture: a registry naming a contract the checker lacks must fail the test"
)]
fn real_owners() -> OwnedCodecs {
    OwnedCodecs::from_owner_registry(
        crate::owned_contract_checks::round_trips(),
        crate::owned_contract_checks::verdicts(),
    )
    .expect("every listed contract is a Router contract")
}

/// Checks frames on one connection of `edge`, each `(sender, frame)`, with the real owners.
fn check(
    edge: OutsideEdge,
    peer: Option<AcpPeer>,
    frames: &[(ProtocolRole, Value)],
) -> Vec<EdgeConformance> {
    let codecs = real_owners();
    let connection = ConnectionId("case".to_owned());
    let mut ledger = ConnectionLedger::default();
    if let Some(peer) = peer {
        ledger.declare_peer(&connection, peer);
    }
    frames
        .iter()
        .map(|(sender, raw)| {
            EdgeDefinitions::shared().check(
                &CheckInput {
                    edge,
                    connection: &connection,
                    sender: *sender,
                    layer: &RecordLayer::WireFrame,
                    answers: None,
                    raw,
                    codecs: &codecs,
                },
                &mut ledger,
            )
        })
        .collect()
}

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

fn missing(result: &EdgeConformance) -> Vec<MissingContext> {
    result
        .leaves()
        .filter_map(|leaf| match leaf {
            EdgeConformance::Incomplete { missing, .. } => Some(missing.clone()),
            _ => None,
        })
        .collect()
}

/// A Claude agent's session/new reply carrying `modelReasoning`.
fn claude_session_new(model_reasoning: Value) -> EdgeConformance {
    check(
        OutsideEdge::AcpAgent,
        Some(AcpPeer::Claude),
        &[
            (
                ProtocolRole::Client,
                json!({"jsonrpc": "2.0", "id": 1, "method": "session/new",
                    "params": {"cwd": "/work", "mcpServers": []}}),
            ),
            (
                ProtocolRole::Server,
                json!({"jsonrpc": "2.0", "id": 1, "result": {"sessionId": "session-1",
                    "_meta": {"codexRouter": {"modelReasoning": model_reasoning}}}}),
            ),
        ],
    )
    .remove(1)
}

#[test]
fn model_metadata_both_owners_reject_is_a_violation() {
    let result = claude_session_new(json!({"version": 1, "kind": "nonsense"}));

    assert!(
        violations(&result)
            .iter()
            .any(|violation| violation.detail == "bothReject"
                && violation.instance_path == "/result/_meta/codexRouter/modelReasoning"),
        "{result:?}"
    );
}

#[test]
fn a_valid_unknown_annotation_with_a_fallback_reason_is_incomplete_not_a_rejection() {
    let result =
        claude_session_new(json!({"version": 1, "kind": "unknown", "reason": "Malformed"}));

    assert!(violations(&result).is_empty(), "{result:?}");
    assert!(
        missing(&result)
            .iter()
            .any(|missing| matches!(missing, MissingContext::OwnerOutcomeUnavailable { .. })),
        "{result:?}"
    );
}

#[test]
fn a_valid_available_annotation_conforms() {
    let result = claude_session_new(json!({"version": 1, "kind": "available", "models": [
        {"kind": "unknown", "modelId": "default", "reason": "Unavailable"}]}));

    assert!(violations(&result).is_empty(), "{result:?}");
    assert!(missing(&result).is_empty(), "{result:?}");
}

/// A Claude agent's available_commands_update with `list_meta` and one command's `command_meta`.
fn claude_commands(list_meta: Value, command_meta: Value) -> EdgeConformance {
    check(
        OutsideEdge::AcpAgent,
        Some(AcpPeer::Claude),
        &[(
            ProtocolRole::Server,
            json!({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": "session-1",
            "update": {
                "sessionUpdate": "available_commands_update",
                "availableCommands": [{"name": "review", "description": "a skill",
                    "_meta": {"codexRouter": command_meta}}],
                "_meta": {"codexRouter": list_meta}
            }}}),
        )],
    )
    .remove(0)
}

#[test]
fn an_available_skill_list_and_skill_command_conform() {
    let result = claude_commands(
        json!({"skillStatus": "available"}),
        json!({"kind": "skill"}),
    );

    assert!(violations(&result).is_empty(), "{result:?}");
    assert!(missing(&result).is_empty(), "{result:?}");
}

#[test]
fn a_skill_list_without_status_is_not_conforming_and_its_other_key_has_no_owner() {
    let result = claude_commands(json!({"effort": 7}), json!({"kind": "skill"}));

    assert!(
        violations(&result)
            .iter()
            .any(|violation| violation.instance_path == "/params/update/_meta/codexRouter"),
        "{result:?}"
    );
    assert!(
        missing(&result).contains(&MissingContext::MissingOwner {
            pointer: "/params/update/_meta/codexRouter/effort".to_owned()
        }),
        "{result:?}"
    );
}

#[test]
fn an_unrecognized_skill_command_kind_is_not_conforming() {
    let result = claude_commands(
        json!({"skillStatus": "available"}),
        json!({"kind": "not-skill"}),
    );

    assert!(
        violations(&result)
            .iter()
            .any(|violation| violation.instance_path
                == "/params/update/availableCommands/0/_meta/codexRouter"),
        "{result:?}"
    );
}

/// N9's session/new reply with `meta`.
fn n9_session_new(meta: Value) -> EdgeConformance {
    check(
        OutsideEdge::AcpServedN9,
        None,
        &[
            (
                ProtocolRole::Client,
                json!({"jsonrpc": "2.0", "id": 1, "method": "session/new",
                    "params": {"cwd": "/work", "mcpServers": []}}),
            ),
            (
                ProtocolRole::Server,
                json!({"jsonrpc": "2.0", "id": 1, "result": {"sessionId": "session-1", "_meta": meta}}),
            ),
        ],
    )
    .remove(1)
}

/// The capability report N9 sends today (from the N9 tap capture).
fn captured_capabilities() -> Value {
    json!({"load": false, "resume": false, "close": false, "list": false, "steer": false,
        "queue": null, "modes": false, "configOptions": false, "elicitation": false,
        "usage": false, "promptContent": {"image": false, "audio": false, "embeddedContext": false},
        "authStatus": {"kind": "notReported"}})
}

#[test]
fn the_real_session_result_owner_accepts_its_shape_and_rejects_history_unavailable() {
    let accepted =
        n9_session_new(json!({"sessionProfile": {"capabilities": captured_capabilities()}}));
    let rejected = n9_session_new(json!({"sessionProfile": {
        "capabilities": captured_capabilities(), "historyUnavailable": false}}));

    assert!(violations(&accepted).is_empty(), "{accepted:?}");
    assert!(
        violations(&rejected)
            .iter()
            .any(|violation| violation.keyword == "routerCodec"
                && violation.instance_path == "/result/_meta"),
        "{rejected:?}"
    );
}

#[test]
fn an_orphan_claude_result_still_runs_its_method_independent_model_owner() {
    // No request on the connection: the method is unknown, but the modelReasoning owner does not
    // depend on it (correction 5, C4-1).
    let result = check(
        OutsideEdge::AcpAgent,
        Some(AcpPeer::Claude),
        &[(
            ProtocolRole::Server,
            json!({"jsonrpc": "2.0", "id": 9, "result": {"sessionId": "session-1",
                "_meta": {"codexRouter": {"modelReasoning": {"version": 1, "kind": "nonsense"}}}}}),
        )],
    )
    .remove(0);

    assert!(
        missing(&result).contains(&MissingContext::OrphanReply { id: json!(9) }),
        "{result:?}"
    );
    assert!(
        violations(&result)
            .iter()
            .any(|violation| violation.detail == "bothReject"
                && violation.instance_path == "/result/_meta/codexRouter/modelReasoning"),
        "{result:?}"
    );
}
