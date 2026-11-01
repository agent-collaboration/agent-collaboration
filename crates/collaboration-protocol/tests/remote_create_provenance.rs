use collaboration_protocol::SessionCreateEntry;
use schemars::schema_for;
use serde_json::{Value, json};

fn human_work_item() -> Value {
    json!({
        "workItemId":"019f0000-0000-7000-8000-000000000001",
        "operationId":"019f0000-0000-7000-8000-000000000002",
        "sourceInstallation":"019f0000-0000-7000-8000-0000000000a1",
        "request":{
            "endpoint":{
                "serviceId":"019f0000-0000-7000-8000-0000000000b2",
                "endpointId":"claude-local"
            },
            "workingDirectory":"/workspace",
            "createdBy":{"humanId":"owner"},
            "approver":null,
            "requestedPolicy":{"access":"workspace-write"},
            "settings":null,
            "firstInput":null
        }
    })
}

#[test]
fn human_work_item_preserves_its_distinct_verified_source() {
    let wire = human_work_item();

    let entry: SessionCreateEntry =
        serde_json::from_value(wire.clone()).expect("attributed human work item");

    assert_eq!(serde_json::to_value(entry).expect("work item JSON"), wire);
}

#[test]
fn work_item_without_verified_source_is_refused() {
    let mut wire = human_work_item();
    wire.as_object_mut().unwrap().remove("sourceInstallation");

    let result = serde_json::from_value::<SessionCreateEntry>(wire);

    assert!(result.is_err(), "an unattributed work item must not decode");
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("sourceInstallation")
    );
}

#[test]
fn source_identity_has_the_same_required_canonical_shape_in_wire_and_schema() {
    let schema = serde_json::to_value(schema_for!(SessionCreateEntry)).expect("work item schema");
    let validator = jsonschema::validator_for(&schema).expect("valid work item schema");
    assert!(validator.is_valid(&human_work_item()));
    for source in [
        json!(null),
        json!(42),
        json!(""),
        json!("not-a-service"),
        json!("019F0000-0000-7000-8000-0000000000A1"),
        json!("019f0000-0000-7000-8000-0000000000a1\n"),
    ] {
        let mut wire = human_work_item();
        wire["sourceInstallation"] = source;
        assert!(serde_json::from_value::<SessionCreateEntry>(wire.clone()).is_err());
        assert!(!validator.is_valid(&wire));
    }
    let mut missing = human_work_item();
    missing
        .as_object_mut()
        .unwrap()
        .remove("sourceInstallation");
    assert!(!validator.is_valid(&missing));
}
