//! Frozen handover values preserve bytes and validate their typed projection.
#[path = "support/automation_import.rs"]
mod fixture;
use fixture::schedule_payload;
use serde_json::{Value, json};

#[test]
fn frozen_payload_keeps_input_bytes_and_freezes_a_canonical_typed_value() {
    use collaboration_protocol::{AutomationImportPayload, FrozenAutomationImportPayload};
    let value = schedule_payload();
    let pretty = serde_json::to_string_pretty(&value).expect("pretty source");
    let parsed =
        FrozenAutomationImportPayload::from_bytes(pretty.as_bytes()).expect("typed source");
    assert_eq!(parsed.as_bytes(), pretty.as_bytes());
    assert_eq!(serde_json::to_value(parsed.payload()).expect("view"), value);
    let typed: AutomationImportPayload =
        serde_json::from_value(value.clone()).expect("payload DTO");
    let canonical = FrozenAutomationImportPayload::freeze(typed).expect("freeze once");
    assert_eq!(canonical.as_bytes(), value.to_string().as_bytes());
    assert_ne!(
        canonical.as_bytes(),
        parsed.as_bytes(),
        "byte identity must not be normalized on input"
    );
}

#[test]
fn request_keeps_exact_string_bytes_through_the_board_value_boundary() {
    use collaboration_protocol::AutomationImportRequest;
    let mut payload = schedule_payload();
    payload["content"]["text"] = json!("literal 雪, quote \", slash \\ and\nline");
    let frozen = serde_json::to_string_pretty(&payload).expect("frozen bytes");
    let request = json!({
        "importId":"019f0000-0000-7000-8000-000000000006","payload":frozen
    });
    let schema = serde_json::to_value(schemars::schema_for!(AutomationImportRequest))
        .expect("request schema");
    assert!(
        jsonschema::validator_for(&schema)
            .expect("schema compiles")
            .is_valid(&request)
    );
    let decoded: AutomationImportRequest =
        serde_json::from_value(request.clone()).expect("request after BoardPayload Value");
    assert_eq!(decoded.payload.as_bytes(), frozen.as_bytes());
    assert_eq!(
        serde_json::to_value(decoded).expect("request roundtrip"),
        request
    );
    for field in ["digest", "sourceInstallation"] {
        let mut invalid = request.clone();
        invalid[field] = json!("caller supplied");
        assert!(serde_json::from_value::<AutomationImportRequest>(invalid).is_err());
    }
    let mut invalid = request;
    invalid["payload"] = payload;
    assert!(serde_json::from_value::<AutomationImportRequest>(invalid).is_err());
}

#[test]
fn suspended_payload_keeps_legacy_timing_without_an_executable_deadline() {
    use collaboration_protocol::FrozenAutomationImportPayload;
    let mut payload = schedule_payload();
    let mut configuration = payload["definition"]["value"].clone();
    configuration
        .as_object_mut()
        .expect("configuration")
        .remove("timing");
    let legacy = json!({"kind":"cron","expression":"58 * 1 * *","timezone":"UTC"});
    payload["definition"] = json!({"kind":"suspended","value":{
        "legacyTiming":legacy,"configuration":{"kind":"schedule","value":configuration}
    }});
    payload["firstDueMs"] = Value::Null;
    let raw = serde_json::to_string_pretty(&payload).expect("suspended source");
    let decoded =
        FrozenAutomationImportPayload::from_bytes(raw.as_bytes()).expect("suspended is distinct");
    assert_eq!(decoded.as_bytes(), raw.as_bytes());
    assert_eq!(
        serde_json::to_value(decoded.payload()).expect("typed view"),
        payload
    );
    let mut illegal_deadline = payload;
    illegal_deadline["firstDueMs"] = json!(1793441100000_i64);
    assert!(
        FrozenAutomationImportPayload::from_bytes(illegal_deadline.to_string().as_bytes()).is_err()
    );
}

#[test]
fn frozen_payload_refuses_unknown_nested_unit_fields_and_inconsistent_progress() {
    use collaboration_protocol::FrozenAutomationImportPayload;
    for (pointer, value) in [
        ("/creator", json!({"kind":"owner","unexpected":true})),
        (
            "/definition/value/destination",
            json!({"kind":"unprepared","unexpected":true}),
        ),
        ("/firstDueMs", json!(1793441400000_i64)),
        ("/externalOccupant", json!("not-a-run-id")),
        (
            "/carriedFiring",
            json!({"firingId":"019f0000-0000-7000-8000-000000000007","dueMs":1793441100000_i64}),
        ),
    ] {
        let mut payload = schedule_payload();
        *payload.pointer_mut(pointer).expect("fixture field") = value;
        assert!(
            FrozenAutomationImportPayload::from_bytes(payload.to_string().as_bytes()).is_err(),
            "invalid {pointer} was accepted"
        );
    }
    for field in [
        "firstDueMs",
        "admittedThroughMs",
        "watermarkLocalMs",
        "externalOccupant",
        "carriedWaitingDueMs",
        "carriedFiring",
    ] {
        let mut payload = schedule_payload();
        payload.as_object_mut().expect("payload").remove(field);
        assert!(
            FrozenAutomationImportPayload::from_bytes(payload.to_string().as_bytes()).is_err(),
            "missing {field} was silently filled"
        );
    }
}

#[test]
fn carried_wake_preserves_its_id_and_inner_message_rules() {
    use collaboration_protocol::FrozenAutomationImportPayload;
    let mut payload = schedule_payload();
    let target = payload["definition"]["value"]["destination"]["target"].clone();
    payload["definition"] = json!({"kind":"wake","value":{
        "configuration":{"target":target,"origin":{"kind":"router"},"delivery":"auto",
            "generationGuard":null,"expiresAt":"2026-10-31T11:00:00Z","state":"active"},
        "timing":{"kind":"at","at":"2026-10-31T10:05:00Z"}
    }});
    let carried =
        json!({"firingId":"019f0000-0000-7000-8000-000000000007","dueMs":1793440800000_i64});
    payload["carriedFiring"] = carried.clone();
    payload["content"]["text"] = json!("literal 雪, quote \", slash \\ and\nline");
    let raw = serde_json::to_string_pretty(&payload).expect("literal wake bytes");
    let decoded = FrozenAutomationImportPayload::from_bytes(raw.as_bytes()).expect("carried wake");
    assert_eq!(decoded.as_bytes(), raw.as_bytes());
    assert_eq!(
        serde_json::to_value(&decoded.payload().carried_firing).expect("carried identity"),
        carried
    );
    let mut invalid = payload.clone();
    invalid["content"]["text"] = json!("forbidden\u{0001}control");
    assert!(FrozenAutomationImportPayload::from_bytes(invalid.to_string().as_bytes()).is_err());
    for field in ["externalOccupant", "carriedWaitingDueMs"] {
        let mut invalid = payload.clone();
        invalid[field] = if field == "externalOccupant" {
            json!("019f0000-0000-7000-8000-000000000008")
        } else {
            json!(1793441100000_i64)
        };
        assert!(FrozenAutomationImportPayload::from_bytes(invalid.to_string().as_bytes()).is_err());
    }
    let mut oversized_counter = payload;
    oversized_counter["definition"]["value"]["configuration"]["generationGuard"] = json!({
        "serviceEpoch":"019f0000-0000-7000-8000-000000000009","generation":9_007_199_254_740_992_u64
    });
    assert!(
        matches!(FrozenAutomationImportPayload::from_bytes(oversized_counter.to_string().as_bytes()),
        Err(collaboration_protocol::AutomationImportPayloadError::Limit(error))
            if error.limit == collaboration_protocol::limits::LimitName::JsonSafeInteger)
    );
}

#[test]
fn encoded_content_and_identity_limits_count_utf8_bytes() {
    use collaboration_protocol::{
        AutomationImportPayloadError, FrozenAutomationImportPayload, limits::LimitName,
    };
    let mut payload = schedule_payload();
    let mut largest = "𐍈\"\\".repeat(174_762);
    largest.push_str("xxxx");
    assert_eq!(largest.len(), 1_048_576);
    payload["content"]["text"] = json!(largest);
    assert!(FrozenAutomationImportPayload::from_bytes(payload.to_string().as_bytes()).is_ok());
    payload["content"]["text"] = json!(format!("{largest}𐍈"));
    assert!(
        matches!(FrozenAutomationImportPayload::from_bytes(payload.to_string().as_bytes()),
        Err(AutomationImportPayloadError::Limit(error)) if error.limit == LimitName::InstructionMaxBytes)
    );
    payload["content"]["text"] = json!("ordinary instruction");
    payload["definition"]["value"]["destination"]["target"]["sessionId"] = json!("𐍈".repeat(1024));
    assert!(FrozenAutomationImportPayload::from_bytes(payload.to_string().as_bytes()).is_ok());
    payload["definition"]["value"]["destination"]["target"]["sessionId"] = json!("𐍈".repeat(1025));
    assert!(
        matches!(FrozenAutomationImportPayload::from_bytes(payload.to_string().as_bytes()),
        Err(AutomationImportPayloadError::Limit(error)) if error.limit == LimitName::SessionIdMaxBytes)
    );
}

#[test]
fn inner_schema_keeps_wake_message_controls_distinct_from_instruction_text() {
    use collaboration_protocol::{AutomationImportPayload, FrozenAutomationImportPayload};
    let schema =
        serde_json::to_value(schemars::schema_for!(AutomationImportPayload)).expect("inner schema");
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    let mut schedule = schedule_payload();
    schedule["content"]["text"] = json!("instruction\u{0001}control");
    assert!(validator.is_valid(&schedule));
    assert!(FrozenAutomationImportPayload::from_bytes(schedule.to_string().as_bytes()).is_ok());
    let target = schedule["definition"]["value"]["destination"]["target"].clone();
    schedule["definition"] = json!({"kind":"wake","value":{
        "configuration":{"target":target,"origin":{"kind":"router"},"delivery":"auto",
            "generationGuard":null,"expiresAt":"2026-10-31T11:00:00Z","state":"active"},
        "timing":{"kind":"at","at":"2026-10-31T10:05:00Z"}
    }});
    assert!(
        !validator.is_valid(&schedule),
        "Wake schema must publish the message-control rule"
    );
    assert!(FrozenAutomationImportPayload::from_bytes(schedule.to_string().as_bytes()).is_err());
}
