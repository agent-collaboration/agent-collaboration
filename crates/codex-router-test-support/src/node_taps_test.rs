use super::{
    BehaviorDefect, CatalogRowExpectation, Deliverable, EdgeParty, ExpectedFailure, MessageFamily,
    NodeStateSnapshot, ProtocolNode, RecordLayer, TapCase, TapOutcome, TapVerdict,
};

/// Session events between parts are typed values, never wire messages.
const TYPED: RecordLayer = RecordLayer::TypedOutcome;

const HUB: EdgeParty = EdgeParty::Node(ProtocolNode::N5EventHub);

fn text_chunk_case() -> TapCase {
    let mut case = TapCase::new(ProtocolNode::N6AcpClient, "agent_text_new_item", "N6/text");
    case.state_before(NodeStateSnapshot::new().field("open_text", "none"));
    case.consumed(
        TYPED,
        MessageFamily::AcpSessionUpdate,
        EdgeParty::AcpAgent,
        "agent_message_chunk",
    );
    case.produced(
        TYPED,
        MessageFamily::SessionEvent,
        HUB,
        "ItemStarted(AgentMessage)",
    );
    case.state_after(NodeStateSnapshot::new().field("open_text", "AgentMessage"));
    case
}

fn text_chunk_row() -> CatalogRowExpectation {
    CatalogRowExpectation::new()
        .produces(
            MessageFamily::SessionEvent,
            HUB,
            "ItemStarted(AgentMessage)",
        )
        .state_after(NodeStateSnapshot::new().field("open_text", "AgentMessage"))
}

#[test]
fn matching_case_passes_and_settles() {
    let result = text_chunk_case().finish_against(&text_chunk_row(), &[]);

    assert_eq!(result.comparison, TapVerdict::MatchesCatalog);
    assert_eq!(result.outcome, TapOutcome::Passed);
    result.assert_settled();
}

#[test]
fn extra_output_is_a_difference() {
    let mut case = text_chunk_case();
    case.produced(
        TYPED,
        MessageFamily::SessionEvent,
        HUB,
        "ItemCompleted(AgentMessage)",
    );

    let result = case.finish_against(&text_chunk_row(), &[]);

    assert_eq!(result.outcome, TapOutcome::UnexpectedFailure);
}

#[test]
fn output_order_is_compared() {
    let row = CatalogRowExpectation::new()
        .produces(
            MessageFamily::SessionEvent,
            HUB,
            "ItemCompleted(AgentMessage)",
        )
        .produces(
            MessageFamily::SessionEvent,
            HUB,
            "ItemStarted(AgentThought)",
        );
    let mut case = TapCase::new(ProtocolNode::N6AcpClient, "kind_switch", "N6/text");
    case.produced(
        TYPED,
        MessageFamily::SessionEvent,
        HUB,
        "ItemStarted(AgentThought)",
    );
    case.produced(
        TYPED,
        MessageFamily::SessionEvent,
        HUB,
        "ItemCompleted(AgentMessage)",
    );

    assert!(matches!(
        case.compare(&row),
        TapVerdict::DiffersFromCatalog(_)
    ));
}

#[test]
fn wrong_counterpart_is_a_difference() {
    let row = CatalogRowExpectation::new().produces(
        MessageFamily::SessionEvent,
        HUB,
        "ItemStarted(AgentMessage)",
    );
    let mut case = TapCase::new(ProtocolNode::N6AcpClient, "wrong_edge", "N6/text");
    case.produced(
        TYPED,
        MessageFamily::SessionEvent,
        EdgeParty::Node(ProtocolNode::N3OutputMapper),
        "ItemStarted(AgentMessage)",
    );

    assert!(matches!(
        case.compare(&row),
        TapVerdict::DiffersFromCatalog(_)
    ));
}

#[test]
fn state_difference_is_a_difference() {
    let mut case = text_chunk_case();
    case.state_after(NodeStateSnapshot::new().field("open_text", "none"));

    let result = case.finish_against(&text_chunk_row(), &[]);

    assert_eq!(result.outcome, TapOutcome::UnexpectedFailure);
}

#[test]
#[should_panic(expected = "differs from its catalog row")]
fn unlisted_failure_panics() {
    let mut case = text_chunk_case();
    case.produced(TYPED, MessageFamily::SessionEvent, HUB, "Notice");

    case.finish_against(&text_chunk_row(), &[]).assert_settled();
}

const LISTED: &[ExpectedFailure] = &[ExpectedFailure {
    case_id: "N6/agent_text_new_item",
    behavior: Some(BehaviorDefect {
        deliverable: Deliverable::D2,
        defect: "listed for the test",
    }),
    schema: &[],
}];

fn notice_defect() -> CatalogRowExpectation {
    CatalogRowExpectation::new()
        .produces(
            MessageFamily::SessionEvent,
            HUB,
            "ItemStarted(AgentMessage)",
        )
        .produces(MessageFamily::SessionEvent, HUB, "Notice")
        .state_after(NodeStateSnapshot::new().field("open_text", "AgentMessage"))
}

#[test]
fn listed_failure_settles_as_expected() {
    let mut case = text_chunk_case();
    case.produced(TYPED, MessageFamily::SessionEvent, HUB, "Notice");
    case.current_defect(notice_defect());

    let result = case.finish_against(&text_chunk_row(), LISTED);

    assert_eq!(
        result.outcome,
        TapOutcome::ExpectedFailure {
            deliverable: Deliverable::D2,
            defect: "listed for the test".to_owned(),
        }
    );
    result.assert_settled();
}

#[test]
#[should_panic(expected = "remove its expected-failure entry")]
fn listed_case_that_passes_panics() {
    text_chunk_case()
        .finish_against(&text_chunk_row(), LISTED)
        .assert_settled();
}

#[test]
fn every_expected_failure_has_a_unique_well_formed_case_id() {
    let mut seen = std::collections::BTreeSet::new();
    for entry in super::EXPECTED_FAILURES
        .iter()
        .flat_map(|group| group.iter())
    {
        let (node, case) = entry
            .case_id
            .split_once('/')
            .unwrap_or_else(|| panic!("{} is not <node>/<case>", entry.case_id));
        assert!(
            node.starts_with('N') && !case.is_empty(),
            "{}",
            entry.case_id
        );
        assert!(
            entry.behavior.is_some() || !entry.schema.is_empty(),
            "{} lists no defect",
            entry.case_id
        );
        assert!(
            entry
                .behavior
                .is_none_or(|behavior| !behavior.defect.is_empty()),
            "{} names no behavior defect",
            entry.case_id
        );
        assert!(seen.insert(entry.case_id), "{} listed twice", entry.case_id);
    }
}

#[test]
#[should_panic(expected = "no longer reproduces its listed defect")]
fn listed_case_failing_another_way_panics() {
    let mut case = text_chunk_case();
    case.current_defect(notice_defect());

    let result = case.finish_against(&CatalogRowExpectation::new(), LISTED);

    assert!(matches!(result.outcome, TapOutcome::DefectChanged { .. }));
    result.assert_settled();
}

#[test]
#[should_panic(expected = "no longer reproduces its listed defect")]
fn listed_case_without_declared_defect_panics() {
    let mut case = text_chunk_case();
    case.produced(TYPED, MessageFamily::SessionEvent, HUB, "Notice");

    case.finish_against(&text_chunk_row(), LISTED)
        .assert_settled();
}

#[test]
#[should_panic(expected = "has no expected-failure entry")]
fn declared_defect_without_entry_panics() {
    let mut case = text_chunk_case();
    case.produced(TYPED, MessageFamily::SessionEvent, HUB, "Notice");
    case.current_defect(notice_defect());

    case.finish_against(&text_chunk_row(), &[]).assert_settled();
}

#[test]
fn payload_values_are_compared_exactly() {
    let row = CatalogRowExpectation::new().produces_with(
        MessageFamily::SessionEvent,
        HUB,
        "ItemStarted(Usage)",
        serde_json::json!({"used": 120, "size": 1000}),
    );
    let mut same = TapCase::new(ProtocolNode::N6AcpClient, "usage", "N6/usage");
    same.produced_with(
        TYPED,
        MessageFamily::SessionEvent,
        HUB,
        "ItemStarted(Usage)",
        serde_json::json!({"used": 120, "size": 1000}),
    );
    assert_eq!(same.compare(&row), TapVerdict::MatchesCatalog);

    let mut corrupted = TapCase::new(ProtocolNode::N6AcpClient, "usage", "N6/usage");
    corrupted.produced_with(
        TYPED,
        MessageFamily::SessionEvent,
        HUB,
        "ItemStarted(Usage)",
        serde_json::json!({"used": 121, "size": 1000}),
    );
    assert!(matches!(
        corrupted.compare(&row),
        TapVerdict::DiffersFromCatalog(_)
    ));

    let mut without_payload = TapCase::new(ProtocolNode::N6AcpClient, "usage", "N6/usage");
    without_payload.produced(
        TYPED,
        MessageFamily::SessionEvent,
        HUB,
        "ItemStarted(Usage)",
    );
    assert!(matches!(
        without_payload.compare(&row),
        TapVerdict::DiffersFromCatalog(_)
    ));
}

#[test]
fn stable_replaces_only_the_named_volatile_fields_at_any_depth() {
    let value = serde_json::json!({
        "itemId": "01a1d9-generated",
        "text": "kept",
        "nested": [{"startedAtMs": 1234, "status": "completed"}],
    });

    let stabilised = super::stable(value, &["itemId", "startedAtMs"]);

    assert_eq!(
        stabilised,
        serde_json::json!({
            "itemId": "<volatile:itemId>",
            "text": "kept",
            "nested": [{"startedAtMs": "<volatile:startedAtMs>", "status": "completed"}],
        })
    );
}
