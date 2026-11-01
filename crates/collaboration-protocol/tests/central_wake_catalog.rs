//! Central wake answers cannot impersonate the local event or delivery owner.
use collaboration_protocol::{AutomationAnswer, CatalogAutomation, CentralWakeSnapshot};
use serde_json::json;

fn central_wake() -> serde_json::Value {
    json!({"definition":{
        "wakeupId":"019f0000-0000-7000-8000-000000000001",
        "changeId":"019f0000-0000-7000-8000-000000000002",
        "createdBy":{"kind":"owner"},
        "message":{"target":{"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"},"sessionId":"target"},
            "content":{"kind":"humanUser","text":"wake"},"delivery":"auto"},
        "timing":{"kind":"at","at":"2026-10-31T10:05:00Z"},
        "anchorAt":"2026-10-31T10:00:00Z","expiresAt":"2026-11-01T10:05:00Z","createdAt":"2026-10-31T10:00:00Z"
    },"state":"active","nextDueAt":"2026-10-31T10:05:00Z","firstFiring":null})
}

#[test]
fn a_populated_central_firing_roundtrips_through_both_wake_variants() {
    let mut value = central_wake();
    value.as_object_mut().expect("Wake object").insert(
        "firstFiring".into(),
        json!({
            "firingId":"019f0000-0000-7000-8000-000000000004",
            "automationId":"019f0000-0000-7000-8000-000000000001",
            "dueAt":1793441100000_i64,
            "revision":"019f0000-0000-7000-8000-000000000002",
            "content":{"contentId":"019f0000-0000-7000-8000-000000000005",
                "revisionId":"019f0000-0000-7000-8000-000000000006","digest":"a".repeat(64)},
            "outcome":{"state":"held"},"reportSeq":null,
            "batchId":"019f0000-0000-7000-8000-000000000007"
        }),
    );
    let schema = serde_json::to_value(schemars::schema_for!(CentralWakeSnapshot)).expect("schema");
    assert!(
        jsonschema::validator_for(&schema)
            .expect("schema compiles")
            .is_valid(&value)
    );
    let snapshot: CentralWakeSnapshot = serde_json::from_value(value.clone()).expect("real Firing");
    assert!(snapshot.first_firing.is_some());
    let expected = json!({"kind":"wake","value":value});
    let answer = serde_json::to_value(AutomationAnswer::Wake(snapshot.clone())).expect("answer");
    assert_eq!(answer, expected);
    let reopened: AutomationAnswer = serde_json::from_value(answer).expect("answer roundtrip");
    assert_eq!(
        serde_json::to_value(reopened).expect("answer serialization"),
        expected
    );
    let catalog = serde_json::to_value(CatalogAutomation::Wake(snapshot)).expect("catalog");
    assert_eq!(catalog, expected);
    let reopened: CatalogAutomation = serde_json::from_value(catalog).expect("catalog roundtrip");
    assert_eq!(
        serde_json::to_value(reopened).expect("catalog serialization"),
        expected
    );
}

#[test]
fn central_wake_uses_actual_firing_custody_without_local_cursor_fields() {
    // Arrange a saved central Wake before its first due time.
    let value = central_wake();
    let snapshot: CentralWakeSnapshot =
        serde_json::from_value(value.clone()).expect("central projection");
    // Act through the public answer envelope.
    let answer = serde_json::to_value(AutomationAnswer::Wake(snapshot)).expect("answer");
    // Assert the new answer and schema agree on explicit absence of a Firing.
    assert_eq!(answer, json!({"kind":"wake","value":value}));
    let schema = serde_json::to_value(schemars::schema_for!(CentralWakeSnapshot)).expect("schema");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    assert!(validator.is_valid(&value));
    for missing in ["firstFiring", "nextDueAt"] {
        let mut corrupt = value.clone();
        corrupt.as_object_mut().expect("object").remove(missing);
        assert!(serde_json::from_value::<CentralWakeSnapshot>(corrupt.clone()).is_err());
        assert!(!validator.is_valid(&corrupt));
    }
    for local_field in ["latestEventCursor", "firstFire", "pendingDeliveryId"] {
        let mut corrupt = value.clone();
        corrupt[local_field] = json!(null);
        assert!(serde_json::from_value::<CentralWakeSnapshot>(corrupt.clone()).is_err());
        assert!(!validator.is_valid(&corrupt));
    }
}
