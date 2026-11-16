//! OB2's recorder cases: exact failure sets, coverage gaps and per-record intent, against the
//! real pinned Codex definition.
use serde_json::{Value, json};

use crate::node_taps::{
    BehaviorDefect, CatalogRowExpectation, Deliverable, EdgeParty, ExpectedFailure, MessageFamily,
    NodeStateSnapshot, ProtocolNode, RecordIntent, RecordKey, RecordLayer, SchemaDefect, TapCase,
    TapCaseResult, TapDirection, TapOutcome,
};

const WIRE: RecordLayer = RecordLayer::WireFrame;
const CLI: EdgeParty = EdgeParty::CodexCli;
const CASE_ID: &str = "N1/compound";

/// A CLI request N1 consumed, with `thread_id` as its `threadId` (a number breaks the schema).
fn turn_start(id: u64, thread_id: &Value) -> Value {
    json!({"id": id, "method": "turn/start", "params": {
        "threadId": thread_id, "input": [{"type": "text", "text": "hello"}]}})
}

/// N1's answer to request `id`, with `status` as the turn's status (not every value is allowed).
fn turn_result(id: u64, status: &str) -> Value {
    json!({"id": id, "result": {"turn": {"id": "turn-1", "items": [], "status": status}}})
}

/// Two turn/start requests and their results, with the given defects in them.
fn compound_case(first_thread: &Value, second_status: &str, produces_notice: bool) -> TapCase {
    let mut case = TapCase::new(ProtocolNode::N1CliFace, "compound", "N1/test");
    let request = MessageFamily::CodexClientRequestFromCli;
    let reply = MessageFamily::CodexMessageToCli;
    case.consumed_raw(
        WIRE,
        request,
        CLI,
        "turn/start",
        turn_start(1, first_thread),
        &[],
    );
    case.produced_raw(
        WIRE,
        reply,
        CLI,
        "response:turn/start",
        turn_result(1, "inProgress"),
        &[],
    );
    case.consumed_raw(
        WIRE,
        request,
        CLI,
        "turn/start",
        turn_start(2, &json!("thread-1")),
        &[],
    );
    case.produced_raw(
        WIRE,
        reply,
        CLI,
        "response:turn/start",
        turn_result(2, second_status),
        &[],
    );
    if produces_notice {
        case.produced(
            RecordLayer::TypedOutcome,
            MessageFamily::SessionCommand,
            EdgeParty::Node(ProtocolNode::N4CommandPort),
            "Notice",
        );
    }
    case.state_after(NodeStateSnapshot::new());
    case
}

/// The catalog row: the two results only.
fn row() -> CatalogRowExpectation {
    let reply = MessageFamily::CodexMessageToCli;
    CatalogRowExpectation::new()
        .produces_with(
            reply,
            CLI,
            "response:turn/start",
            turn_result(1, "inProgress"),
        )
        .produces_with(reply, CLI, "response:turn/start", turn_result(2, "running"))
        .state_after(NodeStateSnapshot::new())
}

/// Today's defect for the case: the row's output plus a stray notice.
fn current_defect() -> CatalogRowExpectation {
    row().produces(
        MessageFamily::SessionCommand,
        EdgeParty::Node(ProtocolNode::N4CommandPort),
        "Notice",
    )
}

const THREAD_ID_DEFECT: SchemaDefect = SchemaDefect {
    deliverable: Deliverable::D1,
    record: RecordKey {
        direction: TapDirection::Consumed,
        kind: "turn/start",
        ordinal: 0,
    },
    schema_path: "/definitions/ClientRequest/oneOf/0/$ref/properties/params/$ref/properties/threadId/type",
    instance_path: "/params/threadId",
    keyword: "type",
    detail: "",
};

const STATUS_DEFECT: SchemaDefect = SchemaDefect {
    deliverable: Deliverable::D1,
    record: RecordKey {
        direction: TapDirection::Produced,
        kind: "response:turn/start",
        ordinal: 1,
    },
    schema_path: "",
    instance_path: "/turn/status",
    keyword: "enum",
    detail: "",
};

/// The case as it really shows, run once to learn the checker's exact schema paths.
fn shown_defects() -> Vec<(String, String, String)> {
    let result = compound_case(&json!(5), "running", true).finish_against(&row(), &[]);
    let mut shown: Vec<(String, String, String)> = result
        .schema
        .unlisted
        .iter()
        .map(|defect| {
            (
                defect.kind.clone(),
                defect.instance_path.clone(),
                defect.schema_path.clone(),
            )
        })
        .collect();
    shown.sort();
    shown
}

/// The listed defects with the real schema paths filled in.
fn listed(defects: &[SchemaDefect]) -> Vec<SchemaDefect> {
    let shown = shown_defects();
    defects
        .iter()
        .map(|defect| {
            let mut defect = *defect;
            if let Some((_, _, path)) = shown.iter().find(|(kind, instance, _)| {
                kind == defect.record.kind && instance == defect.instance_path
            }) {
                defect.schema_path = Box::leak(path.clone().into_boxed_str());
            }
            defect
        })
        .collect()
}

fn entry(behavior: bool, schema: Vec<SchemaDefect>) -> ExpectedFailure {
    ExpectedFailure {
        case_id: CASE_ID,
        behavior: behavior.then_some(BehaviorDefect {
            deliverable: Deliverable::D2,
            defect: "a stray notice",
        }),
        schema: Box::leak(schema.into_boxed_slice()),
    }
}

fn finish(case: TapCase, expected: ExpectedFailure) -> TapCaseResult {
    let mut case = case;
    case.current_defect(current_defect());
    case.finish_against(&row(), &[expected])
}

fn settles(result: &TapCaseResult) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| result.assert_settled())).is_ok()
}

#[test]
fn the_compound_case_shows_two_violations_on_two_messages() {
    let mut instances: Vec<String> = shown_defects()
        .into_iter()
        .map(|(_, instance, _)| instance)
        .collect();
    instances.sort();
    assert_eq!(instances, vec!["/params/threadId", "/turn/status"]);
}

#[test]
fn the_exact_set_settles() {
    let result = finish(
        compound_case(&json!(5), "running", true),
        entry(true, listed(&[THREAD_ID_DEFECT, STATUS_DEFECT])),
    );
    assert!(
        matches!(result.outcome, TapOutcome::ExpectedFailure { .. }),
        "{:?}",
        result.outcome
    );
    assert_eq!(result.schema.expected.len(), 2);
    assert!(settles(&result), "{:#?}", result.schema);
}

#[test]
fn an_unlisted_second_violation_fails_on_its_own() {
    let result = finish(
        compound_case(&json!(5), "running", true),
        entry(true, listed(&[THREAD_ID_DEFECT])),
    );
    assert_eq!(result.schema.unlisted.len(), 1);
    assert_eq!(result.schema.unlisted[0].instance_path, "/turn/status");
    assert!(!settles(&result));
}

#[test]
fn a_partly_fixed_set_fails_naming_the_fix() {
    // The second result is now valid; its listed defect must go.
    let result = finish(
        compound_case(&json!(5), "inProgress", true),
        entry(true, listed(&[THREAD_ID_DEFECT, STATUS_DEFECT])),
    );
    assert_eq!(result.schema.fixed.len(), 1);
    assert_eq!(
        result.schema.fixed[0].identity.instance_path,
        "/turn/status"
    );
    assert!(result.schema.unlisted.is_empty());
    assert!(!settles(&result));
}

#[test]
fn the_same_violation_on_another_message_of_the_kind_fails() {
    // The violation moves from the second result (ordinal 1) to the first (ordinal 0).
    let mut case = TapCase::new(ProtocolNode::N1CliFace, "compound", "N1/test");
    let request = MessageFamily::CodexClientRequestFromCli;
    let reply = MessageFamily::CodexMessageToCli;
    case.consumed_raw(
        WIRE,
        request,
        CLI,
        "turn/start",
        turn_start(1, &json!(5)),
        &[],
    );
    case.produced_raw(
        WIRE,
        reply,
        CLI,
        "response:turn/start",
        turn_result(1, "running"),
        &[],
    );
    case.consumed_raw(
        WIRE,
        request,
        CLI,
        "turn/start",
        turn_start(2, &json!("thread-1")),
        &[],
    );
    case.produced_raw(
        WIRE,
        reply,
        CLI,
        "response:turn/start",
        turn_result(2, "inProgress"),
        &[],
    );
    case.produced(
        RecordLayer::TypedOutcome,
        MessageFamily::SessionCommand,
        EdgeParty::Node(ProtocolNode::N4CommandPort),
        "Notice",
    );
    let result = finish(
        case,
        entry(true, listed(&[THREAD_ID_DEFECT, STATUS_DEFECT])),
    );
    assert_eq!(result.schema.unlisted.len(), 1);
    assert_eq!(result.schema.unlisted[0].ordinal, 0);
    assert_eq!(result.schema.fixed.len(), 1);
    assert!(!settles(&result));
}

#[test]
fn a_behavior_regression_beside_listed_violations_fails() {
    // The stray notice is gone, but the behavior defect is still listed.
    let result = finish(
        compound_case(&json!(5), "running", false),
        entry(true, listed(&[THREAD_ID_DEFECT, STATUS_DEFECT])),
    );
    assert!(result.schema.is_settled());
    assert!(
        matches!(
            result.outcome,
            TapOutcome::UnexpectedPass { .. } | TapOutcome::DefectChanged { .. }
        ),
        "{:?}",
        result.outcome
    );
    assert!(!settles(&result));
}

#[test]
fn a_wire_record_without_its_raw_value_is_a_coverage_gap() {
    let mut case = TapCase::new(ProtocolNode::N1CliFace, "gap", "N1/test");
    case.consumed(
        WIRE,
        MessageFamily::CodexClientRequestFromCli,
        CLI,
        "turn/start",
    );
    let result = case.finish_against(&CatalogRowExpectation::new(), &[]);
    assert_eq!(result.schema.coverage_gaps.len(), 1, "{:?}", result.schema);
    assert!(!settles(&result));

    let mut declared = TapCase::new(ProtocolNode::N1CliFace, "gap", "N1/test");
    declared.declare_coverage_gap("turn/start", "the test sees only the decoded request");
    declared.consumed(
        WIRE,
        MessageFamily::CodexClientRequestFromCli,
        CLI,
        "turn/start",
    );
    let result = declared.finish_against(&CatalogRowExpectation::new(), &[]);
    assert!(result.schema.coverage_gaps.is_empty());
    assert_eq!(result.schema.declared_gaps.len(), 1);
    assert!(settles(&result));
}

#[test]
fn an_orphan_reply_is_a_coverage_gap_unless_it_names_what_it_answers() {
    let mut case = TapCase::new(ProtocolNode::N1CliFace, "orphan", "N1/test");
    case.produced_raw(
        WIRE,
        MessageFamily::CodexMessageToCli,
        CLI,
        "response",
        turn_result(9, "inProgress"),
        &[],
    );
    let row = CatalogRowExpectation::new().produces_with(
        MessageFamily::CodexMessageToCli,
        CLI,
        "response",
        turn_result(9, "inProgress"),
    );
    let result = case.finish_against(&row, &[]);
    assert_eq!(result.schema.coverage_gaps.len(), 1);

    let mut answered = TapCase::new(ProtocolNode::N1CliFace, "orphan", "N1/test");
    answered.produced_raw(
        WIRE,
        MessageFamily::CodexMessageToCli,
        CLI,
        "response",
        turn_result(9, "inProgress"),
        &[],
    );
    answered.last_answers("turn/start");
    let result = answered.finish_against(&row, &[]);
    assert!(settles(&result), "{:#?}", result.schema);
}

/// One injected invalid CLI request, plus N1's own reply with an unrelated defect.
fn injected_case(product_status: &str, injected_thread: &Value) -> TapCase {
    let mut case = TapCase::new(ProtocolNode::N1CliFace, "compound", "N1/test");
    case.consumed_raw(
        WIRE,
        MessageFamily::CodexClientRequestFromCli,
        CLI,
        "turn/start",
        turn_start(1, injected_thread),
        &[],
    );
    case.mark_last(RecordIntent::DeliberateInvalid {
        keyword: "type",
        instance_path: "/params/threadId",
        detail: "",
    });
    case.produced_raw(
        WIRE,
        MessageFamily::CodexMessageToCli,
        CLI,
        "response:turn/start",
        turn_result(1, product_status),
        &[],
    );
    case
}

fn injected_row(status: &str) -> CatalogRowExpectation {
    CatalogRowExpectation::new().produces_with(
        MessageFamily::CodexMessageToCli,
        CLI,
        "response:turn/start",
        turn_result(1, status),
    )
}

#[test]
fn an_injected_violation_exempts_only_itself() {
    let clean =
        injected_case("inProgress", &json!(5)).finish_against(&injected_row("inProgress"), &[]);
    assert!(settles(&clean), "{:#?}", clean.schema);

    let unrelated =
        injected_case("running", &json!(5)).finish_against(&injected_row("running"), &[]);
    assert_eq!(unrelated.schema.unlisted.len(), 1);
    assert_eq!(unrelated.schema.unlisted[0].instance_path, "/turn/status");
    assert!(!settles(&unrelated));
}

#[test]
fn an_injection_that_did_not_inject_fails() {
    let result = injected_case("inProgress", &json!("thread-1"))
        .finish_against(&injected_row("inProgress"), &[]);
    assert_eq!(result.schema.injections_not_shown.len(), 1);
    assert!(!settles(&result));
}

/// One CLI turn/start request with `params`, answered by an error so it pairs.
fn bare_turn_start(params: Value) -> TapCase {
    let mut case = TapCase::new(ProtocolNode::N1CliFace, "compound", "N1/test");
    case.consumed_raw(
        WIRE,
        MessageFamily::CodexClientRequestFromCli,
        CLI,
        "turn/start",
        json!({"id": 1, "method": "turn/start", "params": params}),
        &[],
    );
    case
}

/// The missing-property defects a bare turn/start case shows, by their `required` detail.
fn missing_properties(params: Value) -> Vec<String> {
    let result = bare_turn_start(params).finish_against(&CatalogRowExpectation::new(), &[]);
    let mut details: Vec<String> = result
        .schema
        .unlisted
        .iter()
        .filter(|defect| defect.keyword == "required")
        .map(|defect| defect.detail.clone())
        .collect();
    details.sort();
    details
}

#[test]
fn two_missing_properties_are_two_defects() {
    assert_eq!(missing_properties(json!({})), vec!["input", "threadId"]);
    assert_eq!(
        missing_properties(json!({"threadId": "thread-a"})),
        vec!["input"]
    );
    assert_eq!(missing_properties(json!({"input": []})), vec!["threadId"]);
}

#[test]
fn a_partial_fix_of_two_missing_properties_names_what_disappeared() {
    let shown = bare_turn_start(json!({})).finish_against(&CatalogRowExpectation::new(), &[]);
    let listed: Vec<SchemaDefect> = shown
        .schema
        .unlisted
        .iter()
        .map(|defect| SchemaDefect {
            deliverable: Deliverable::D1,
            record: RecordKey {
                direction: TapDirection::Consumed,
                kind: "turn/start",
                ordinal: 0,
            },
            schema_path: Box::leak(defect.schema_path.clone().into_boxed_str()),
            instance_path: Box::leak(defect.instance_path.clone().into_boxed_str()),
            keyword: Box::leak(defect.keyword.clone().into_boxed_str()),
            detail: Box::leak(defect.detail.clone().into_boxed_str()),
        })
        .collect();
    assert_eq!(listed.len(), 2);
    let entry = ExpectedFailure {
        case_id: CASE_ID,
        behavior: None,
        schema: Box::leak(listed.into_boxed_slice()),
    };
    let both = bare_turn_start(json!({})).finish_against(&CatalogRowExpectation::new(), &[entry]);
    assert!(settles(&both), "{:#?}", both.schema);
    let partly = bare_turn_start(json!({"threadId": "thread-a"}))
        .finish_against(&CatalogRowExpectation::new(), &[entry]);
    let fixed: Vec<&str> = partly
        .schema
        .fixed
        .iter()
        .map(|defect| defect.identity.detail.as_str())
        .collect();
    assert_eq!(fixed, vec!["threadId"]);
    assert!(!settles(&partly));
}

#[test]
fn a_setup_frame_is_checked_but_not_compared() {
    let mut case = TapCase::new(ProtocolNode::N1CliFace, "setup", "N1/test");
    case.produced_raw(
        WIRE,
        MessageFamily::CodexMessageToCli,
        CLI,
        "turn/started",
        json!({"method": "turn/started", "params": {"threadId": 5}}),
        &[],
    );
    case.mark_last_setup();
    let result = case.finish_against(&CatalogRowExpectation::new(), &[]);
    assert_eq!(
        result.comparison,
        crate::node_taps::TapVerdict::MatchesCatalog
    );
    assert!(
        result
            .schema
            .unlisted
            .iter()
            .any(|defect| defect.instance_path == "/params/threadId"),
        "a setup frame is still checked: {:?}",
        result.schema.unlisted
    );
}

#[test]
fn an_injection_exempts_only_its_declared_detail_on_the_same_record() {
    // {} against TurnStartParams misses both input and threadId; only threadId was injected.
    let mut case = bare_turn_start(json!({}));
    case.mark_last(RecordIntent::DeliberateInvalid {
        keyword: "required",
        instance_path: "/params",
        detail: "threadId",
    });
    let result = case.finish_against(&CatalogRowExpectation::new(), &[]);
    let unlisted: Vec<&str> = result
        .schema
        .unlisted
        .iter()
        .map(|defect| defect.detail.as_str())
        .collect();
    assert_eq!(
        unlisted,
        vec!["input"],
        "the undeclared missing property stays a defect"
    );
    assert!(!settles(&result));
}
