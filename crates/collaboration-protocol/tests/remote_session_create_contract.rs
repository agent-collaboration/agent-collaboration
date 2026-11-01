//! Central status distinguishes unreported work from an observed uncertain effect.
use collaboration_protocol::{AutomationAnswer, RemoteSessionCreateRequest, SessionCreateEntry};
use serde_json::{Value, json};

fn unreported_snapshot() -> Value {
    json!({
        "workItemId":"019f0000-0000-7000-8000-000000000001",
        "operationId":"019f0000-0000-7000-8000-000000000002",
        "state":"pending","create":null,"input":{"state":"none"},"reportSeq":null
    })
}

fn request_definition() -> Value {
    json!({
        "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"},
        "workingDirectory":"/fixture",
        "createdBy":{"humanId":"fixture owner"},
        "approver":null,"requestedPolicy":{"access":"workspace-write"},
        "settings":null,"firstInput":null
    })
}

#[test]
fn status_roundtrips_custody_and_requires_explicit_unreported_fields() {
    let schema = serde_json::to_value(schemars::schema_for!(AutomationAnswer)).expect("schema");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    for state in ["pending", "held", "received"] {
        let mut snapshot = unreported_snapshot();
        snapshot["state"] = json!(state);
        let answer = json!({"kind":"sessionCreate","value":snapshot});
        assert!(validator.is_valid(&answer), "schema omitted {state} status");
        let decoded: AutomationAnswer = serde_json::from_value(answer.clone()).expect("status");
        assert_eq!(serde_json::to_value(decoded).expect("roundtrip"), answer);
    }
    for missing in ["create", "reportSeq"] {
        let mut snapshot = unreported_snapshot();
        snapshot.as_object_mut().expect("snapshot").remove(missing);
        let corrupt = json!({"kind":"sessionCreate","value":snapshot});
        assert!(!validator.is_valid(&corrupt));
        assert!(serde_json::from_value::<AutomationAnswer>(corrupt).is_err());
    }
    let mut unknown = unreported_snapshot();
    unknown["state"] = json!("unknown");
    let corrupt = json!({"kind":"sessionCreate","value":unknown});
    assert!(!validator.is_valid(&corrupt));
    assert!(serde_json::from_value::<AutomationAnswer>(corrupt).is_err());
}

#[test]
fn observed_uncertainty_is_a_typed_positive_sequence_report() {
    let mut snapshot = unreported_snapshot();
    snapshot["state"] = json!("received");
    snapshot["create"] = json!({"state":"unknown"});
    snapshot["input"] = json!({"state":"unknown"});
    snapshot["reportSeq"] = json!(1);
    let answer = json!({"kind":"sessionCreate","value":snapshot});
    let schema = serde_json::to_value(schemars::schema_for!(AutomationAnswer)).expect("schema");
    assert!(
        jsonschema::validator_for(&schema)
            .expect("schema compiles")
            .is_valid(&answer)
    );
    let decoded: AutomationAnswer =
        serde_json::from_value(answer.clone()).expect("reported status");
    assert_eq!(serde_json::to_value(decoded).expect("roundtrip"), answer);
    let mut zero = answer;
    zero["value"]["reportSeq"] = json!(0);
    assert!(serde_json::from_value::<AutomationAnswer>(zero).is_err());
}

#[test]
fn verified_source_is_required_on_the_entry_and_refused_on_the_request() {
    let definition = request_definition();
    let operation = "019f0000-0000-7000-8000-000000000002";
    let source = "019f0000-0000-7000-8000-000000000004";
    let entry = json!({
        "workItemId":"019f0000-0000-7000-8000-000000000001",
        "operationId":operation,"sourceInstallation":source,"request":definition
    });
    let schema = serde_json::to_value(schemars::schema_for!(SessionCreateEntry)).expect("schema");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    assert!(
        validator.is_valid(&entry),
        "entry schema refused verified source: {:?}",
        validator.validate(&entry)
    );
    let decoded: SessionCreateEntry = serde_json::from_value(entry.clone()).expect("bound entry");
    assert_eq!(serde_json::to_value(decoded).expect("roundtrip"), entry);
    let mut missing = entry;
    missing
        .as_object_mut()
        .expect("entry")
        .remove("sourceInstallation");
    assert!(!validator.is_valid(&missing));
    assert!(serde_json::from_value::<SessionCreateEntry>(missing).is_err());

    let request = json!({"operationId":operation,"definition":definition});
    assert!(serde_json::from_value::<RemoteSessionCreateRequest>(request.clone()).is_ok());
    let mut spoofed = request;
    spoofed["sourceInstallation"] = json!(source);
    assert!(serde_json::from_value::<RemoteSessionCreateRequest>(spoofed).is_err());
}
