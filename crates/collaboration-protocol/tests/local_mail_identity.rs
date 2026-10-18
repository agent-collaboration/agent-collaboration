//! MS-2 / PD4 4a: optional mail identity is validated before admission.
use collaboration_protocol::SessionMessageSendParams;
use serde_json::json;

#[test]
fn supplied_mail_identity_round_trips_and_malformed_identity_is_refused() {
    let base = json!({"target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"target"},"message":{"kind":"humanUser","text":"identity control"}});
    let id = "01900000-0000-7000-8000-000000000001";
    let mut supplied = base.clone();
    supplied["itemId"] = json!(id);
    let decoded = serde_json::from_value::<SessionMessageSendParams>(supplied.clone());
    assert!(
        decoded.is_ok(),
        "a valid supplied identity was refused: {decoded:?}"
    );
    let encoded = serde_json::to_value(decoded.unwrap()).unwrap();
    assert_eq!(encoded["itemId"], id);
    assert!(serde_json::from_value::<SessionMessageSendParams>(base).is_ok());
    for invalid in [
        "",
        "opaque",
        "01900000-0000-4000-8000-000000000001",
        "01900000-0000-7000-0000-000000000001",
        "01900000-0000-7000-8000-00000000000A",
    ] {
        supplied["itemId"] = json!(invalid);
        assert!(
            serde_json::from_value::<SessionMessageSendParams>(supplied.clone()).is_err(),
            "accepted malformed itemId {invalid}"
        );
    }
}
