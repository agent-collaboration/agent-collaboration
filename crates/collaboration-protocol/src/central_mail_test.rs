use super::{
    CentralMailOperation, CentralMailRequest, CentralMailResult, MailPolicy, MessageBody,
    MessageSender, MessageState,
};
use serde_json::json;
use std::num::NonZeroU32;

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
fn policy_has_default_and_accepts_nonzero_duration_without_upper_bound() {
    assert_eq!(MailPolicy::default().window_seconds(), 900);
    assert_eq!(MailPolicy::new(NonZeroU32::MAX).window_seconds(), u32::MAX);
}
