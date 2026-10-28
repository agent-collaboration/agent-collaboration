use super::*;
use schemars::generate::SchemaSettings;
use serde_json::{Value, json};

const LOCAL_SEND: &str = r#"{"pushId":"01900000-0000-7000-8000-000000000001","link":"router://00000000-0000-4000-8000-000000000001/push/01900000-0000-7000-8000-000000000001","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"recipient"},"targetIdentity":"recipient label","deliveryState":"delivered","receipt":{"outcome":{"kind":"peerMessageWritten"},"reachability":null,"client":null}}"#;
const LOCAL_REPLY: &str = r#"{"target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"recipient"},"targetIdentity":"recipient label","pushId":"01900000-0000-7000-8000-000000000001","link":"router://00000000-0000-4000-8000-000000000001/push/01900000-0000-7000-8000-000000000001","deliveryState":"delivered","receipt":{"outcome":{"kind":"peerMessageWritten"},"reachability":null,"client":null}}"#;
const QUEUED: &str = r#"{"queued":{"item":"01900000-0000-7000-8000-000000000002","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"codex-local"},"sessionId":"remote-recipient"},"targetIdentity":"remote label"}}"#;

#[test]
fn shared_catalog_keeps_both_local_receipt_schemas_under_distinct_result_names() {
    let send_name = <MessageSendResult as JsonSchema>::schema_name();
    let reply_name = <MessageReplyResult as JsonSchema>::schema_name();
    assert_ne!(
        send_name, reply_name,
        "generic result schemas overwrite each other"
    );
    let catalog = crate::protocol_type_schemas().unwrap();
    let send = catalog.get(send_name.as_ref()).unwrap();
    let reply = catalog.get(reply_name.as_ref()).unwrap();
    assert_eq!(send["oneOf"][0]["$ref"], "#/$defs/PushMessageSendResult");
    assert_eq!(
        reply["oneOf"][0]["$ref"],
        "#/$defs/SessionMessageReplyResult"
    );
}

#[test]
fn local_send_json_remains_byte_identical_without_an_added_tag() {
    // Arrange: the literal old local receipt is the independent wire oracle.
    let local: PushMessageSendResult = serde_json::from_str(LOCAL_SEND).unwrap();
    assert_eq!(serde_json::to_string(&local).unwrap(), LOCAL_SEND);
    // Act/Assert: an untagged Local preserves every byte, including native receipt fields.
    let result = MessageSendResult::Local(local);
    assert_eq!(serde_json::to_string(&result).unwrap(), LOCAL_SEND);
    assert!(matches!(
        serde_json::from_str::<MessageSendResult>(LOCAL_SEND).unwrap(),
        MessageSendResult::Local(_)
    ));
}

#[test]
fn local_reply_json_keeps_its_original_order_and_receipt() {
    let local: SessionMessageReplyResult = serde_json::from_str(LOCAL_REPLY).unwrap();
    assert_eq!(serde_json::to_string(&local).unwrap(), LOCAL_REPLY);
    let result: MessageReplyResult = MessageSendResult::Local(local);
    assert_eq!(serde_json::to_string(&result).unwrap(), LOCAL_REPLY);
    assert!(matches!(
        serde_json::from_str::<MessageReplyResult>(LOCAL_REPLY).unwrap(),
        MessageSendResult::Local(_)
    ));
}

#[test]
fn queued_json_contains_only_item_and_frozen_target_without_a_native_receipt_or_push() {
    let result: MessageSendResult = serde_json::from_str(QUEUED).unwrap();
    assert!(matches!(&result, MessageSendResult::Queued { queued }
        if queued.item.as_str() == "01900000-0000-7000-8000-000000000002"
            && queued.target_identity == "remote label"));
    assert_eq!(serde_json::to_string(&result).unwrap(), QUEUED);
    let reply: MessageReplyResult = serde_json::from_str(QUEUED).unwrap();
    assert_eq!(serde_json::to_string(&reply).unwrap(), QUEUED);
}

#[test]
fn actual_send_and_reply_schemas_use_disjoint_one_of_and_reject_invented_fields() {
    let schemas = [
        serde_json::to_value(
            SchemaSettings::draft2020_12()
                .for_serialize()
                .into_generator()
                .into_root_schema_for::<MessageSendResult>(),
        )
        .unwrap(),
        serde_json::to_value(
            SchemaSettings::draft2020_12()
                .for_serialize()
                .into_generator()
                .into_root_schema_for::<MessageReplyResult>(),
        )
        .unwrap(),
    ];
    let queued: Value = serde_json::from_str(QUEUED).unwrap();
    let local_send: Value = serde_json::from_str(LOCAL_SEND).unwrap();
    let local_reply: Value = serde_json::from_str(LOCAL_REPLY).unwrap();
    let mut with_receipt = queued.clone();
    with_receipt["queued"]["receipt"] = json!({"outcome":{"kind":"queued"}});
    let mut with_push = queued.clone();
    with_push["pushId"] = json!("01900000-0000-7000-8000-000000000002");
    let mut missing_item = queued.clone();
    missing_item["queued"]
        .as_object_mut()
        .unwrap()
        .remove("item");
    let mut wrong_item = queued.clone();
    wrong_item["queued"]["item"] = json!("not-an-item");
    let mut missing_label = queued.clone();
    missing_label["queued"]
        .as_object_mut()
        .unwrap()
        .remove("targetIdentity");
    let invalid = [
        with_receipt,
        with_push,
        missing_item,
        wrong_item,
        missing_label,
        json!({"kind":"local","value":local_send}),
        json!({"queued":null}),
        json!({}),
    ];
    for schema in schemas {
        assert_eq!(schema["oneOf"].as_array().unwrap().len(), 2);
        assert!(schema.get("anyOf").is_none());
        let validator = jsonschema::validator_for(&schema).unwrap();
        for valid in [&queued, &local_send, &local_reply] {
            assert!(validator.is_valid(valid), "valid literal refused: {valid}");
        }
        for value in &invalid {
            assert!(!validator.is_valid(value), "schema accepted {value}");
            assert!(
                serde_json::from_value::<MessageSendResult>(value.clone()).is_err(),
                "send decoder accepted {value}"
            );
            assert!(
                serde_json::from_value::<MessageReplyResult>(value.clone()).is_err(),
                "reply decoder accepted {value}"
            );
        }
    }
}
