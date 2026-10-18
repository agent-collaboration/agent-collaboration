use message_board::{ReaderState, RequestDecision, RequestResult, WriteReply};
use serde_json::json;

#[test]
fn cooldown_wire_code_is_shared_by_gate_and_board_errors() {
    assert_eq!(
        serde_json::to_value(message_board::BoardFailureKind::TopLevelPostCooldown).unwrap(),
        json!("postCooldown")
    );
    let refusal = json!({"kind":"postCooldown","reason":{"retryAfterSeconds":1}});
    let typed: message_board::Refusal = serde_json::from_value(refusal.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), refusal);
    assert!(
        serde_json::from_value::<message_board::BoardFailureKind>(json!("topLevelPostCooldown"))
            .is_err()
    );
    assert!(
        serde_json::from_value::<message_board::Refusal>(
            json!({"kind":"topLevelPostCooldown","reason":{"retryAfterSeconds":1}})
        )
        .is_err()
    );
}

#[test]
fn unread_reader_state_is_outside_the_stored_decision() {
    let reply = json!({"result":{"kind":"decided","decision":{"kind":"accepted","records":[],"positions":[],"change":"alreadySo"}},"reader":{"subject":{"kind":"project","id":"01890f2e-7b4c-7cc0-98c4-000000000001"},"readThrough":{"epoch":1,"position":3},"unreadCount":1,"unread":[{"kind":"task","id":"01890f2e-7b4c-7cc0-98c4-000000000002"}]}});
    let parsed: WriteReply = serde_json::from_value(reply.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), reply);
    assert!(serde_json::from_value::<RequestDecision>(json!({"kind":"accepted","records":[],"positions":[],"change":"alreadySo","reader":null})).is_err());
}
#[test]
fn fleet_reply_has_no_project_reader_state() {
    let reply = WriteReply {
        result: RequestResult::ConflictingRequest,
        reader: None,
    };
    assert_eq!(
        serde_json::to_value(reply).unwrap(),
        json!({"result":{"kind":"conflictingRequest"},"reader":null})
    );
}
#[test]
fn reader_state_and_reply_shapes_reject_unknown_fields() {
    assert!(
        serde_json::from_value::<WriteReply>(
            json!({"result":{"kind":"overloaded"},"reader":null,"extra":true})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<ReaderState>(json!({"subject":{"kind":"project","id":"01890f2e-7b4c-7cc0-98c4-000000000001"},"readThrough":{"epoch":1,"position":0},"unreadCount":0,"unread":[],"extra":true})).is_err());
}
