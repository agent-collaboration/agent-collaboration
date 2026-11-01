//! Preparation binds one explicit owned thread, without accepting another destination mode.
use collaboration_protocol::AutomationCall;
use serde_json::{Value, json};

fn request() -> Value {
    json!({"kind":"scheduleBindPrepared","value":{
        "automationId":"019f0000-0000-7000-8000-000000000001",
        "expectedRevision":"019f0000-0000-7000-8000-000000000002",
        "operationId":"019f0000-0000-7000-8000-000000000003",
        "boundThread":{"target":{
            "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000004","endpointId":"codex-local"},
            "sessionId":"prepared-thread"
        },"cwd":"/workspace"}
    }})
}

#[test]
fn prepared_binding_roundtrips_the_exact_typed_call() {
    let request = request();
    let schema = serde_json::to_value(schemars::schema_for!(AutomationCall)).expect("schema");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    assert!(
        validator.is_valid(&request),
        "prepared binding missing from schema"
    );
    let decoded: AutomationCall = serde_json::from_value(request.clone()).expect("typed binding");
    assert_eq!(serde_json::to_value(decoded).expect("roundtrip"), request);
}

#[test]
fn prepared_binding_requires_all_fields_and_refuses_arbitrary_destinations() {
    let schema = serde_json::to_value(schemars::schema_for!(AutomationCall)).expect("schema");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    for field in [
        "automationId",
        "expectedRevision",
        "operationId",
        "boundThread",
    ] {
        let mut missing = request();
        missing["value"]
            .as_object_mut()
            .expect("request")
            .remove(field);
        assert!(!validator.is_valid(&missing));
        assert!(serde_json::from_value::<AutomationCall>(missing).is_err());
    }
    for thread in [
        json!({"kind":"unprepared"}),
        json!({"kind":"freshEachRun","endpoint":{
            "serviceId":"019f0000-0000-7000-8000-000000000004","endpointId":"codex-local"
        },"cwd":"/workspace"}),
        json!({"target":request()["value"]["boundThread"]["target"]}),
        json!({"target":request()["value"]["boundThread"]["target"],"cwd":"/workspace","extra":true}),
    ] {
        let mut invalid = request();
        invalid["value"]["boundThread"] = thread;
        assert!(!validator.is_valid(&invalid));
        assert!(serde_json::from_value::<AutomationCall>(invalid).is_err());
    }
}
