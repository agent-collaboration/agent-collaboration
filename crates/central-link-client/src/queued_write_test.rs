use super::*;

#[test]
fn a_tool_cannot_submit_a_timestamp_and_only_the_claim_adds_it() {
    let mut value = serde_json::json!({
        "to": {
            "machineId": "019f0000-0000-7000-8000-00000000c402",
            "sessionId": "recipient",
        },
        "body": "message text",
        "delivery": "queue",
        "replyTo": null,
    });
    let input: SendMessageRequest = serde_json::from_value(value.clone()).unwrap();
    value.as_object_mut().unwrap().insert(
        "sentAt".to_owned(),
        serde_json::json!("2030-01-01T00:00:01Z"),
    );
    assert!(serde_json::from_value::<SendMessageRequest>(value).is_err());
    let queued = QueuedWrite::SendMessage {
        request_id: "019f0000-0000-7000-8000-00000000c401".parse().unwrap(),
        message: input,
    };
    assert!(!serde_json::to_string(&queued).unwrap().contains("sentAt"));
    let stamp = "2030-01-01T00:02:03Z".parse().unwrap();
    let RequestPayload::Mail(wire) = queued.into_wire(stamp) else {
        panic!("the queued message produced a mail request");
    };
    assert!(
        matches!(wire.operation, CentralMailOperation::SendMessage { sent_at, .. } if sent_at == stamp)
    );
}
