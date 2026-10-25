use message_board::BoardOperation;
use serde_json::json;

#[test]
fn project_close_requires_its_reason_and_revision_without_a_producer_position() {
    let input = json!({"kind":"projectClose","reason":"work complete","expectedRevision":4});
    let decoded = serde_json::from_value::<BoardOperation>(input.clone());
    assert!(
        decoded.is_ok(),
        "authoritative close operation: {decoded:?}"
    );
    for field in ["reason", "expectedRevision"] {
        let mut missing = input.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<BoardOperation>(missing).is_err());
    }
    let mut producer_position = input;
    producer_position["position"] = json!({"epoch":1,"position":41});
    assert!(serde_json::from_value::<BoardOperation>(producer_position).is_err());
}
