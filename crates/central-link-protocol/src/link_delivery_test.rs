use super::*;
use serde_json::json;

#[test]
fn delivery_vocabulary_has_its_closed_wire_names_and_send_ids_are_uuidv7() {
    let outcomes = [
        (MessageOutcome::Delivered, json!({"kind": "delivered"})),
        (
            MessageOutcome::Refused {
                reason: MessageRefusal::RecipientRefused,
            },
            json!({"kind": "refused", "reason": "recipientRefused"}),
        ),
        (
            MessageOutcome::Cancelled {
                reason: MessageCancellation::NotDeliveredWithin15Min,
            },
            json!({"kind": "cancelled", "reason": "notDeliveredWithin15Min"}),
        ),
    ];

    for (outcome, wire) in outcomes {
        assert_eq!(serde_json::to_value(&outcome).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<MessageOutcome>(wire).unwrap(),
            outcome
        );
    }
    assert_eq!(
        serde_json::to_value(DeliveryAnswer::RefusedForGood).unwrap(),
        json!("refusedForGood")
    );
    assert!(
        serde_json::from_value::<SendId>(json!("019f0000-0000-7000-8000-000000000001")).is_ok()
    );
    assert!(
        serde_json::from_value::<SendId>(json!("019f0000-0000-4000-8000-000000000001")).is_err()
    );
    assert!(serde_json::from_value::<MessageOutcome>(json!({"kind": "expired"})).is_err());
}
