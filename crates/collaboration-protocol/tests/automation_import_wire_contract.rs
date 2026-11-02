//! Import call encoding is distinct from frozen-payload construction.
use collaboration_protocol::AutomationCall;
#[path = "support/automation_import.rs"]
mod fixture;
use fixture::schedule_payload;
use serde_json::json;

#[test]
fn import_call_preserves_the_frozen_json_string_through_value_roundtrips() {
    let frozen = serde_json::to_string_pretty(&schedule_payload()).expect("frozen fixture");
    let call = json!({"kind":"automationImport","value":{
        "importId":"019f0000-0000-7000-8000-000000000006","payload":frozen
    }});
    let schema = serde_json::to_value(schemars::schema_for!(AutomationCall)).expect("call schema");
    assert!(
        jsonschema::validator_for(&schema)
            .expect("schema compiles")
            .is_valid(&call),
        "published call schema omitted the bound handover contract"
    );
    let decoded: AutomationCall = serde_json::from_value(call.clone()).expect("handover call");
    assert_eq!(serde_json::to_value(decoded).expect("roundtrip"), call);
}

#[test]
fn import_request_never_accepts_a_caller_digest_source_or_object_payload() {
    let frozen = schedule_payload().to_string();
    for request in [
        json!({"importId":"019f0000-0000-7000-8000-000000000006","payload":frozen,"digest":"0".repeat(64)}),
        json!({"importId":"019f0000-0000-7000-8000-000000000006","payload":frozen,"sourceInstallation":"019f0000-0000-7000-8000-000000000004"}),
        json!({"importId":"019f0000-0000-7000-8000-000000000006","payload":schedule_payload()}),
    ] {
        let call = json!({"kind":"automationImport","value":request});
        assert!(serde_json::from_value::<AutomationCall>(call).is_err());
    }
}
