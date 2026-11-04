//! Distinct protocol vocabularies have identical delivery intent and validation at their boundary.
use collaboration_mail_protocol::{GenerationGuard, MailDeliveryMode};
use collaboration_protocol::{CodexGeneration, MessageDelivery};
use serde_json::{Value, json};

#[test]
fn delivery_tags_and_default_round_trip_without_changing_json() {
    for (rpc, mail, wire) in [
        (MessageDelivery::Auto, MailDeliveryMode::Auto, "auto"),
        (MessageDelivery::Queue, MailDeliveryMode::Queue, "queue"),
        (MessageDelivery::Steer, MailDeliveryMode::Steer, "steer"),
    ] {
        assert_eq!(MailDeliveryMode::from(rpc), mail);
        assert_eq!(MessageDelivery::from(mail), rpc);
        assert_eq!(serde_json::to_value(rpc).unwrap(), json!(wire));
        assert_eq!(serde_json::to_value(mail).unwrap(), json!(wire));
    }
    assert_eq!(
        MailDeliveryMode::from(MessageDelivery::default()),
        MailDeliveryMode::default()
    );
    assert_eq!(
        serde_json::to_value(MailDeliveryMode::default()).unwrap(),
        json!("auto")
    );
    for invalid in [json!("defer"), json!({"kind":"auto"}), Value::Null] {
        assert!(serde_json::from_value::<MessageDelivery>(invalid.clone()).is_err());
        assert!(serde_json::from_value::<MailDeliveryMode>(invalid).is_err());
    }
}

#[test]
fn generation_guard_round_trips_exact_json_and_rejects_the_same_invalid_inputs() {
    for generation in [1, 9_007_199_254_740_991_u64] {
        let wire =
            json!({"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":generation});
        let rpc: CodexGeneration = serde_json::from_value(wire.clone()).unwrap();
        let mail: GenerationGuard = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(GenerationGuard::try_from(rpc.clone()).unwrap(), mail);
        assert_eq!(CodexGeneration::try_from(mail.clone()).unwrap(), rpc);
        assert_eq!(serde_json::to_value(rpc).unwrap(), wire);
        assert_eq!(serde_json::to_value(mail).unwrap(), wire);
    }
    let valid = json!({"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1});
    let mut invalid_inputs = Vec::new();
    for number in [
        json!(0),
        json!(9_007_199_254_740_992_u64),
        json!(-1),
        json!(1.5),
    ] {
        let mut wire = valid.clone();
        wire["generation"] = number;
        invalid_inputs.push(wire);
    }
    for epoch in [
        "00000000-0000-4000-8000-00000000000A",
        "00000000000040008000000000000001",
        "not-a-uuid",
    ] {
        let mut wire = valid.clone();
        wire["serviceEpoch"] = json!(epoch);
        invalid_inputs.push(wire);
    }
    let mut wrong_spelling = valid.clone();
    wrong_spelling
        .as_object_mut()
        .unwrap()
        .remove("serviceEpoch");
    wrong_spelling["service_epoch"] = json!("00000000-0000-4000-8000-000000000001");
    invalid_inputs.push(wrong_spelling);
    let mut unknown = valid;
    unknown["extra"] = json!(true);
    invalid_inputs.push(unknown);
    for wire in invalid_inputs {
        assert!(
            serde_json::from_value::<CodexGeneration>(wire.clone()).is_err(),
            "RPC accepted {wire}"
        );
        assert!(
            serde_json::from_value::<GenerationGuard>(wire.clone()).is_err(),
            "mail accepted {wire}"
        );
    }
}

#[test]
fn session_references_convert_both_ways_without_reinterpreting_provider_ids() {
    for session_id in ["provider:opaque/thread#1".to_owned(), "μ".repeat(2048)] {
        let wire = json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":session_id});
        let rpc: collaboration_protocol::SessionRef = serde_json::from_value(wire.clone()).unwrap();
        let mail = message_board::SessionRef::try_from(rpc.clone()).unwrap();
        assert_eq!(serde_json::to_value(&mail).unwrap(), wire);
        assert_eq!(
            collaboration_protocol::SessionRef::try_from(mail).unwrap(),
            rpc
        );
    }
    let valid = json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"opaque"});
    for (field, value) in [
        ("/sessionId", json!("")),
        ("/sessionId", json!("invalid\u{0000}")),
        ("/sessionId", json!("a".repeat(4097))),
        ("/endpoint/endpointId", json!("../codex")),
        (
            "/endpoint/serviceId",
            json!("00000000-0000-4000-8000-00000000000A"),
        ),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(field).unwrap() = value;
        assert!(
            serde_json::from_value::<collaboration_protocol::SessionRef>(invalid.clone()).is_err()
        );
        assert!(serde_json::from_value::<message_board::SessionRef>(invalid).is_err());
    }
    for location in ["", "/endpoint"] {
        let mut invalid = valid.clone();
        invalid.pointer_mut(location).unwrap()["extra"] = json!(true);
        assert!(
            serde_json::from_value::<collaboration_protocol::SessionRef>(invalid.clone()).is_err()
        );
        assert!(serde_json::from_value::<message_board::SessionRef>(invalid).is_err());
    }
}
