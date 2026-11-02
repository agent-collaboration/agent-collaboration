//! Portable imports keep a closed package shape and always disable in the catalog.
use collaboration_protocol::AutomationCall;
use serde_json::{Value, json};

fn request() -> Value {
    json!({"kind":"schedulePackageImport","value":{
        "operationId":"019f0000-0000-7000-8000-000000000001","overwrite":false,
        "package":{
            "scheduleId":"019f0000-0000-7000-8000-000000000002",
            "sourceChangeId":"source-change",
            "instruction":{"instructionId":"019f0000-0000-7000-8000-000000000003",
                "text":"portable instructions","sourceRevisionId":"source-revision"},
            "definition":{"instructionId":"019f0000-0000-7000-8000-000000000003",
                "timing":{"kind":"interval","seconds":600},
                "enabled":false,"destination":{"kind":"unprepared"},
                "executionTimeoutSeconds":null,"model":null,"effort":"low"},
            "continuity":null
        }
    }})
}

#[test]
fn package_import_roundtrips_both_unprepared_modes_and_optional_continuity() {
    let schema = serde_json::to_value(schemars::schema_for!(AutomationCall)).expect("schema");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    for mode in ["unprepared", "freshEachRunUnprepared"] {
        for continuity in [
            json!(null),
            json!({"text":"imported summary","sourceRunId":"source-run",
            "sourceTarget":{"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000004","endpointId":"codex-local"},"sessionId":"summary-source"}}),
        ] {
            let mut request = request();
            request["value"]["package"]["definition"]["destination"]["kind"] = json!(mode);
            request["value"]["package"]["definition"]["model"] = json!("fixture-model");
            request["value"]["package"]["continuity"] = continuity;
            assert!(
                validator.is_valid(&request),
                "package import schema refused {mode}: {:?}",
                validator
                    .validate(&request)
                    .map_err(|error| error.to_string())
            );
            let decoded: AutomationCall = serde_json::from_value(request.clone()).expect("package");
            assert_eq!(serde_json::to_value(decoded).expect("roundtrip"), request);
        }
    }
}

#[test]
fn package_import_refuses_extra_fields_and_empty_source_provenance() {
    let schema = serde_json::to_value(schemars::schema_for!(AutomationCall)).expect("schema");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    for pointer in [
        "/value/disabled",
        "/value/package/extra",
        "/value/package/instruction/extra",
    ] {
        let mut invalid = request();
        let (parent, field) = pointer.rsplit_once('/').expect("field");
        invalid
            .pointer_mut(parent)
            .expect("object")
            .as_object_mut()
            .expect("fields")
            .insert(field.into(), json!(true));
        assert!(!validator.is_valid(&invalid));
        assert!(serde_json::from_value::<AutomationCall>(invalid).is_err());
    }
    for pointer in [
        "/value/package/sourceChangeId",
        "/value/package/instruction/sourceRevisionId",
    ] {
        let mut invalid = request();
        *invalid.pointer_mut(pointer).expect("provenance") = json!("");
        assert!(!validator.is_valid(&invalid));
        assert!(serde_json::from_value::<AutomationCall>(invalid).is_err());
    }
}
