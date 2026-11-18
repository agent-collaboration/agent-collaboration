use super::{
    CentralMailOperation, CentralMailRequest, CentralMailResult, MessageBody, MessageCancellation,
    MessageDelivery, MessageOutcome, MessageRefusal, MessageSender, MessageState,
};
use serde_json::json;

#[test]
fn request_round_trips_typed_timing_and_camel_case_fields() {
    let wire = json!({
        "requestId": "01890f3e-7b8c-7cc0-8000-000000000001",
        "operation": {
            "kind": "setAutomation",
            "target": {
                "machineId": "01890f3e-7b8c-7cc0-8000-000000000002",
                "endpoint": "codex",
                "sessionId": "session-1"
            },
            "timing": { "kind": "interval", "seconds": 60 },
            "text": "remind me"
        }
    });
    let request: CentralMailRequest = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        request.operation,
        CentralMailOperation::SetAutomation { .. }
    ));
    assert_eq!(serde_json::to_value(request).unwrap(), wire);
}

#[test]
fn rejects_invalid_ids_unknown_fields_and_missing_variant_payloads() {
    let invalid = [
        json!({"requestId":"550e8400-e29b-41d4-a716-446655440000", "operation":{"kind":"cancelWake", "wakeId":"01890f3e-7b8c-7cc0-8000-000000000001"}}),
        json!({"requestId":"01890f3e-7b8c-7cc0-8000-000000000001", "operation":{"kind":"cancelWake", "wakeId":"550e8400-e29b-41d4-a716-446655440000"}}),
        json!({"requestId":"01890f3e-7b8c-7cc0-8000-000000000001", "operation":{"kind":"cancelWake", "wakeId":"01890f3e-7b8c-7cc0-8000-000000000002", "extra":true}}),
        json!({"requestId":"01890f3e-7b8c-7cc0-8000-000000000001", "operation":{"kind":"cancelWake"}}),
        json!({"requestId":"01890f3e-7b8c-7cc0-8000-000000000001", "operation":{"kind":"unknown"}}),
    ];
    for wire in invalid {
        assert!(serde_json::from_value::<CentralMailRequest>(wire).is_err());
    }
    assert!(serde_json::from_value::<MessageState>(json!({"kind":"failed"})).is_err());
    assert!(
        serde_json::from_value::<MessageBody>(json!({"kind":"notice", "items":[], "extra":true}))
            .is_err()
    );
}

#[test]
fn nested_result_preserves_both_discriminators() {
    let wire = json!({"kind":"done", "value":{"kind":"wakeCancelled", "wakeId":"01890f3e-7b8c-7cc0-8000-000000000001"}});
    let result: CentralMailResult = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(result).unwrap(), wire);
    assert!(
        serde_json::from_value::<CentralMailResult>(
            json!({"kind":"done", "value":{"kind":"unknown"}})
        )
        .is_err()
    );
}

#[test]
fn sender_variants_are_closed_and_round_trip() {
    for wire in [
        json!({"kind":"actor", "value":"01890f3e-7b8c-7cc0-8000-000000000001"}),
        json!({"kind":"central"}),
    ] {
        let sender: MessageSender = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(sender).unwrap(), wire);
    }
    for wire in [
        json!({"kind":"actor"}),
        json!({"kind":"central", "value":"01890f3e-7b8c-7cc0-8000-000000000001"}),
        json!({"kind":"unknown"}),
    ] {
        assert!(serde_json::from_value::<MessageSender>(wire).is_err());
    }
}

#[test]
fn queue_has_no_time_cutoff_and_ready_is_not_a_message_state() {
    let wire = json!({"session":{"machineId":"01890f3e-7b8c-7cc0-8000-000000000002","endpoint":"codex","sessionId":"session-1"},"messages":[]});
    let queue: super::SessionQueue = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(queue).unwrap(), wire);
    let mut obsolete = wire;
    obsolete["through"] = json!("2030-01-01T00:00:00Z");
    assert!(serde_json::from_value::<super::SessionQueue>(obsolete).is_err());
    assert!(serde_json::from_value::<MessageState>(json!({"kind":"ready"})).is_err());
}

#[test]
fn notice_cadence_request_is_closed_and_round_trips_each_allowed_value() {
    for cadence in ["m15", "m25", "m55"] {
        let wire = json!({"requestId":"01890f3e-7b8c-7cc0-8000-000000000001","operation":{"kind":"setNoticeCadence","cadence":cadence}});
        let parsed: CentralMailRequest = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
    }
    for cadence in ["m20", "15", "", "M15"] {
        let wire = json!({"requestId":"01890f3e-7b8c-7cc0-8000-000000000001","operation":{"kind":"setNoticeCadence","cadence":cadence}});
        assert!(serde_json::from_value::<CentralMailRequest>(wire).is_err());
    }
}

#[test]
fn a_direct_body_or_send_without_delivery_or_reply_to_is_refused_not_defaulted() {
    // Hard cutover: no stored or sent message predates these fields.
    let bodies = [
        json!({ "kind": "direct", "body": "hello" }),
        json!({ "kind": "direct", "body": "hello", "delivery": "auto" }),
        json!({ "kind": "direct", "body": "hello", "replyTo": null }),
    ];
    let to = json!({"machineId": "019f0000-0000-7000-8000-000000000001", "endpoint": "codex-local", "sessionId": "s"});
    let sends = [
        json!({ "kind": "sendMessage", "to": to, "body": "hello" }),
        json!({ "kind": "sendMessage", "to": to, "body": "hello", "delivery": "auto" }),
    ];

    for stored in bodies {
        assert!(
            serde_json::from_value::<MessageBody>(stored.clone()).is_err(),
            "{stored}"
        );
    }
    for send in sends {
        assert!(
            serde_json::from_value::<CentralMailOperation>(send.clone()).is_err(),
            "{send}"
        );
    }
}

#[test]
fn a_direct_body_keeps_its_chosen_delivery() {
    let stored = json!({ "kind": "direct", "body": "hello", "delivery": "steer", "replyTo": null });
    let body: MessageBody = serde_json::from_value(stored.clone()).unwrap();
    assert!(matches!(
        body,
        MessageBody::Direct {
            delivery: MessageDelivery::Steer,
            ..
        }
    ));
    assert_eq!(serde_json::to_value(body).unwrap(), stored);
}

#[test]
fn a_direct_body_keeps_the_message_it_answers() {
    let stored = json!({
        "kind": "direct",
        "body": "answer",
        "delivery": "auto",
        "replyTo": "01890f3e-7b8c-7cc0-8000-000000000009"
    });
    let body: MessageBody = serde_json::from_value(stored.clone()).unwrap();
    assert!(
        matches!(&body, MessageBody::Direct { reply_to: Some(id), .. } if id.as_str() == "01890f3e-7b8c-7cc0-8000-000000000009")
    );
    assert_eq!(serde_json::to_value(body).unwrap(), stored);
}

#[test]
fn a_delivery_outcome_body_has_closed_wire_names_and_refuses_unknown_outcomes() {
    let outcomes = [
        (
            MessageOutcome::DeliveredAfterAll,
            json!({"kind": "deliveredAfterAll"}),
        ),
        (
            MessageOutcome::Refused {
                reason: MessageRefusal::RecipientRefused,
            },
            json!({"kind": "refused", "reason": "recipientRefused"}),
        ),
        (
            MessageOutcome::Cancelled {
                reason: MessageCancellation::NotDeliveredWithin15Minutes,
            },
            json!({"kind": "cancelled", "reason": "notDeliveredWithin15Minutes"}),
        ),
    ];
    for (outcome, outcome_wire) in outcomes {
        let wire = json!({
            "kind": "deliveryOutcome",
            "messageId": "01890f3e-7b8c-7cc0-8000-000000000001",
            "requestId": "01890f3e-7b8c-7cc0-8000-000000000002",
            "outcome": outcome_wire,
        });
        let body: MessageBody = serde_json::from_value(wire.clone()).unwrap();
        assert!(
            matches!(body, MessageBody::DeliveryOutcome { outcome: decoded, .. } if decoded == outcome)
        );
        assert_eq!(serde_json::to_value(body).unwrap(), wire);
    }
    for outcome in [json!({"kind": "delivered"}), json!({"kind": "expired"})] {
        assert!(
            serde_json::from_value::<MessageBody>(json!({
                "kind": "deliveryOutcome",
                "messageId": "01890f3e-7b8c-7cc0-8000-000000000001",
                "requestId": "01890f3e-7b8c-7cc0-8000-000000000002",
                "outcome": outcome,
            }))
            .is_err()
        );
    }
}
