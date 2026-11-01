use board_sync_protocol::BoardNotice;
use serde_json::json;

#[test]
fn automation_batch_hint_retains_its_uuid_v7_identity_and_rejects_invalid_payloads() {
    let value = json!({"kind":"automationBatchReady","value":{"batchId":"019f0000-0000-7000-8000-000000000001"}});
    let notice: BoardNotice = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(notice).unwrap(), value);
    for invalid in [
        json!({"kind":"automationBatchReady","value":{"batchId":"019f0000-0000-4000-8000-000000000001"}}),
        json!({"kind":"automationBatchReady","value":{"batchId":"019f0000-0000-7000-8000-000000000001","text":"not a hint"}}),
    ] {
        assert!(serde_json::from_value::<BoardNotice>(invalid).is_err());
    }
}
