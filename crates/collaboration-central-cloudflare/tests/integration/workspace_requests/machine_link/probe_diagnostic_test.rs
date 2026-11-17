use super::{DiagnosticRefusal, INPUT_PREFIX, RESULT_PREFIX, collect, result_marker};
use serde_json::{Value, json};

const INSTANCE: &str = "019f0000-0000-7000-8000-00000000df01";
const OBJECT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn valid_input() -> Value {
    let attachment = json!({
        "organizationId":"019f0000-0000-7000-8000-00000000df02",
        "machineId":"019f0000-0000-7000-8000-00000000df03",
        "installationId":"019f0000-0000-7000-8000-00000000df04",
        "enrollmentId":"019f0000-0000-7000-8000-00000000df05",
        "clientId":"019f0000-0000-7000-8000-00000000df06","effectiveExpiresAt":1_900_000_600,
    });
    json!({"stage":"eviction","disposition":"active-references","events":[
        {"sequence":1,"phase":"connecting","instanceId":INSTANCE,"event":{"kind":"constructor","objectId":OBJECT}},
        {"sequence":2,"phase":"connecting","instanceId":INSTANCE,"event":{"kind":"upgrade","objectId":OBJECT,"instanceId":INSTANCE,"attachments":[attachment]}},
        {"sequence":3,"phase":"baseline","instanceId":INSTANCE,"event":{"kind":"handler","objectId":OBJECT,"instanceId":INSTANCE,"attachment":attachment}},
        {"sequence":4,"phase":"baseline","instanceId":INSTANCE,"event":{"kind":"alarmSnapshot","objectId":OBJECT,"correlationId":"019f0000-0000-7000-8000-00000000df07","earliestDueMs":null,"alarmMs":null}}
    ]})
}

fn line(value: &Value) -> String {
    format!("{INPUT_PREFIX}{value}")
}

fn replaced(pointer: &str, replacement: Value) -> String {
    let mut value = valid_input();
    if let Some(field) = value.pointer_mut(pointer) {
        *field = replacement;
    }
    line(&value)
}

fn parsed_result(logs: &[String]) -> Option<Value> {
    let marker = result_marker(collect(true, true, logs), true)
        .ok()
        .flatten()?;
    serde_json::from_str(marker.strip_prefix(RESULT_PREFIX)?).ok()
}

#[test]
fn valid_diagnostic_is_reserialized_as_only_the_allowlisted_disposed_result() {
    // Arrange / Act: unrelated logs are never forwarded into the result marker.
    let input = valid_input();
    let result = parsed_result(&["unrelated private fixture log".into(), line(&input)]);
    let mut expected = input;
    if let Some(object) = expected.as_object_mut() {
        object.insert("disposed".into(), json!(true));
    }
    // Assert: exact producer schema, with no raw line or log field.
    assert_eq!(result, Some(expected));
}

#[test]
fn zero_or_multiple_anchored_diagnostics_cannot_produce_a_marker() {
    assert_eq!(
        result_marker(collect(true, true, &[]), true),
        Err(DiagnosticRefusal)
    );
    let logs = vec![line(&valid_input()), line(&valid_input())];
    assert_eq!(
        result_marker(collect(true, true, &logs), true),
        Err(DiagnosticRefusal)
    );
    assert!(
        collect(
            true,
            true,
            &[format!("not anchored {}", line(&valid_input()))]
        )
        .is_err()
    );
}

#[test]
fn unknown_fields_are_rejected_at_every_nested_boundary() {
    for pointer in [
        "",
        "/events/0",
        "/events/0/event",
        "/events/1/event/attachments/0",
        "/events/2/event/attachment",
    ] {
        let mut value = valid_input();
        if let Some(object) = value.pointer_mut(pointer).and_then(Value::as_object_mut) {
            object.insert("extra".into(), json!(true));
        }
        assert!(
            parsed_result(&[line(&value)]).is_none(),
            "unknown field at {pointer}"
        );
    }
}

#[test]
fn malformed_json_stage_and_disposition_return_only_a_safe_refusal() {
    for encoded in [
        format!("{INPUT_PREFIX}not-json-private-token"),
        replaced("/stage", json!("request")),
        replaced("/disposition", json!("passed")),
    ] {
        let result = collect(true, true, &[encoded]);
        assert_eq!(result, Err(DiagnosticRefusal));
    }
    assert_eq!(
        DiagnosticRefusal.to_string(),
        "ordinary accept probe evidence is unavailable or invalid"
    );
}

#[test]
fn malformed_witness_ids_and_sequences_do_not_survive_reserialization() {
    for encoded in [
        replaced(
            "/events/0/instanceId",
            json!("019f0000-0000-4000-8000-00000000df01"),
        ),
        replaced(
            "/events/0/instanceId",
            json!("019f0000-0000-7000-0000-00000000df01"),
        ),
        replaced("/events/0/event/objectId", json!("A".repeat(64))),
        replaced("/events/0/event/objectId", json!("abc")),
        replaced(
            "/events/1/event/instanceId",
            json!("019f0000-0000-7000-8000-00000000df11"),
        ),
        replaced("/events/0/sequence", json!(0)),
    ] {
        assert!(parsed_result(&[encoded]).is_none());
    }
}

#[test]
fn malformed_or_incomplete_attachment_identity_is_rejected() {
    for field in [
        "organizationId",
        "machineId",
        "installationId",
        "enrollmentId",
        "clientId",
    ] {
        let pointer = format!("/events/1/event/attachments/0/{field}");
        assert!(
            parsed_result(&[replaced(
                &pointer,
                json!("019f0000-0000-4000-8000-00000000df01")
            )])
            .is_none()
        );
    }
    let mut missing = valid_input();
    if let Some(attachment) = missing
        .pointer_mut("/events/2/event/attachment")
        .and_then(Value::as_object_mut)
    {
        attachment.remove("enrollmentId");
    }
    assert!(parsed_result(&[line(&missing)]).is_none());
}

#[test]
fn negative_and_unsafe_attachment_or_alarm_times_are_rejected() {
    for pointer in [
        "/events/1/event/attachments/0/effectiveExpiresAt",
        "/events/2/event/attachment/effectiveExpiresAt",
        "/events/3/event/earliestDueMs",
        "/events/3/event/alarmMs",
    ] {
        for time in [-1_i64, 9_007_199_254_740_992] {
            assert!(
                parsed_result(&[replaced(pointer, json!(time))]).is_none(),
                "unsafe time at {pointer}"
            );
        }
    }
}

#[test]
fn normal_mode_successful_journeys_and_failed_disposal_emit_no_result_marker() {
    let logs = vec![format!("{INPUT_PREFIX}malformed-private-log")];
    assert_eq!(result_marker(collect(false, true, &logs), true), Ok(None));
    assert_eq!(result_marker(collect(true, false, &logs), true), Ok(None));
    assert_eq!(
        result_marker(collect(true, true, &[line(&valid_input())]), false),
        Err(DiagnosticRefusal)
    );
}

#[test]
fn unclassified_eviction_is_preserved_as_evidence_without_becoming_a_pass() {
    let input = replaced("/disposition", json!("unclassified-eviction-failure"));
    let output = parsed_result(&[input]);
    assert_eq!(
        output.and_then(|value| value.get("disposition").cloned()),
        Some(json!("unclassified-eviction-failure"))
    );
}
