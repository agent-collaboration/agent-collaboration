use message_board::{Participant, PendingRootNotice};
use serde_json::json;

#[test]
fn participant_interval_positions_are_qualified_by_the_project_epoch() {
    let value = json!({"identity":{"kind":"human","humanId":"reader"},"role":"participant","note":null,"joinedAtPosition":{"epoch":1,"position":3},"lastSeenPosition":{"epoch":2,"position":8},"closedAtPosition":null,"closedReason":null,"replacedBy":null});
    let parsed = serde_json::from_value::<Participant>(value.clone());
    assert!(parsed.is_ok(), "qualified participant interval: {parsed:?}");
    assert_eq!(serde_json::to_value(parsed.unwrap()).unwrap(), value);
}
#[test]
fn subscription_locator_is_a_project_cursor_range_with_a_thread_identity() {
    let value = json!({"threadId":"01890f2e-7b4c-7cc0-98c4-000000000001","projectId":"01890f2e-7b4c-7cc0-98c4-000000000002","from":{"epoch":1,"position":41},"through":{"epoch":1,"position":42},"postCount":2});
    let parsed = serde_json::from_value::<PendingRootNotice>(value.clone());
    assert!(parsed.is_ok(), "qualified notice: {parsed:?}");
    assert_eq!(serde_json::to_value(parsed.unwrap()).unwrap(), value);
}
