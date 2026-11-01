//! The catalog answer and refusal wire shapes are shared with the Router cutover.
use collaboration_protocol::{
    AutomationAnswer, AutomationFault, AutomationRefusal, CatalogAutomation,
    FaultedAutomationSnapshot,
};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn validate_schema_and_roundtrip<TRecord: JsonSchema + Serialize + DeserializeOwned>(
    value: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let schema = serde_json::to_value(schemars::schema_for!(TRecord))?;
    if !jsonschema::validator_for(&schema)?.is_valid(&value) {
        return Err(format!("schema refused {value}").into());
    }
    let decoded: TRecord = serde_json::from_value(value.clone())?;
    let encoded = serde_json::to_value(decoded)?;
    if encoded != value {
        return Err(format!("roundtrip changed {value} to {encoded}").into());
    }
    Ok(())
}

#[test]
fn instruction_and_revision_answers_publish_their_distinct_record_pages() {
    let instruction = json!({
        "instructionId":"019f0000-0000-7000-8000-000000000001",
        "revisionId":"019f0000-0000-7000-8000-000000000002",
        "text":"catalog instruction",
        "createdAt":"2026-10-31T10:00:00Z",
        "updatedAt":"2026-10-31T10:05:00Z"
    });
    let revision = json!({
        "instructionId":"019f0000-0000-7000-8000-000000000001",
        "revisionId":"019f0000-0000-7000-8000-000000000002",
        "recordedAt":"2026-10-31T10:05:00Z",
        "sourceRevisionId":null,
        "text":"catalog instruction"
    });
    for (kind, record) in [("instructions", instruction), ("revisions", revision)] {
        for next_cursor in [json!(null), json!("opaque-next-page")] {
            validate_schema_and_roundtrip::<AutomationAnswer>(json!({
                "kind":kind,"value":{"records":[record],"nextCursor":next_cursor}
            }))
            .expect("populated catalog page");
        }
        validate_schema_and_roundtrip::<AutomationAnswer>(json!({
            "kind":kind,"value":{"records":[],"nextCursor":null}
        }))
        .expect("empty catalog page");
    }
}

#[test]
fn faulted_snapshots_remain_typed_in_show_and_catalog_without_a_definition() {
    let fault = json!({
        "error":{"kind":"invalidRecord","field":"definition_json"},
        "faultedAt":"2026-10-31T10:05:00Z"
    });
    validate_schema_and_roundtrip::<AutomationFault>(fault.clone()).expect("typed fault");
    for kind in ["schedule", "wake"] {
        let snapshot = json!({
            "automationId":"019f0000-0000-7000-8000-000000000003",
            "kind":kind,
            "revision":"019f0000-0000-7000-8000-000000000004",
            "fault":fault
        });
        validate_schema_and_roundtrip::<FaultedAutomationSnapshot>(snapshot.clone())
            .expect("faulted snapshot");
        let envelope = json!({"kind":"faulted","value":snapshot});
        validate_schema_and_roundtrip::<AutomationAnswer>(envelope.clone()).expect("Show fault");
        validate_schema_and_roundtrip::<CatalogAutomation>(envelope).expect("catalog fault");
    }
}

#[test]
fn six_catalog_refusals_publish_and_validate_their_typed_payloads() {
    let valid = [
        json!({"kind":"instructionRevisionConflict","currentRevisionId":"019f0000-0000-7000-8000-000000000002"}),
        json!({"kind":"scheduleChangeConflict","currentChangeId":"019f0000-0000-7000-8000-000000000004"}),
        json!({"kind":"invalidRecord","field":"definition_json"}),
        json!({"kind":"invalidRequest","field":"cwd","reason":"absolutePathRequired"}),
        json!({"kind":"wakeExpiryRequired"}),
        json!({"kind":"wakeLifecycleConflict","state":"expired"}),
    ];
    for refusal in valid {
        validate_schema_and_roundtrip::<AutomationRefusal>(refusal.clone()).expect("typed refusal");
        validate_schema_and_roundtrip::<AutomationAnswer>(json!({
            "kind":"refused","value":refusal
        }))
        .expect("refusal answer");
    }

    let schema = serde_json::to_value(schemars::schema_for!(AutomationRefusal)).expect("schema");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    for malformed in [
        json!({"kind":"instructionRevisionConflict","currentRevisionId":"not-a-revision"}),
        json!({"kind":"scheduleChangeConflict","currentChangeId":17}),
        json!({"kind":"invalidRecord","field":17}),
        json!({"kind":"invalidRequest","field":"cwd","reason":"unknownConstraint"}),
        json!({"wakeExpiryRequired":true}),
        json!({"kind":"wakeLifecycleConflict","state":"unknownState"}),
    ] {
        assert!(
            serde_json::from_value::<AutomationRefusal>(malformed.clone()).is_err(),
            "decoder accepted {malformed}"
        );
        assert!(
            !validator.is_valid(&malformed),
            "schema accepted {malformed}"
        );
    }
}
