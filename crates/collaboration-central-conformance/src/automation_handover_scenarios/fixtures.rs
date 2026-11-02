//! Shared frozen payloads and checked observations for central handover proof.
use crate::ConformanceResult;
use serde_json::{Value, json};

pub(super) fn import_request(payload: &str) -> Value {
    json!({"kind":"automationImport","value":{
        "importId":durable_steps::Uuid::now_v7().to_string(),"payload":payload
    }})
}

pub(super) fn schedule_payload(target: &str) -> Value {
    json!({
        "automationId":durable_steps::Uuid::now_v7().to_string(),
        "revision":durable_steps::Uuid::now_v7().to_string(),
        "creator":{"kind":"owner"},
        "header":{"anchorAt":"2026-10-31T09:00:00Z","createdAt":"2026-10-31T08:00:00Z",
            "updatedAt":"2026-10-31T09:30:00Z","importedContinuity":null},
        "definition":{"kind":"schedule","value":{
            "instructionId":durable_steps::Uuid::now_v7().to_string(),
            "timing":{"kind":"at","at":"2026-10-31T10:05:00Z"},"enabled":true,
            "destination":{"kind":"ownedThread","target":{"endpoint":{"serviceId":target,
                "endpointId":"codex-local"},"sessionId":"central-handover-fixture"},"cwd":"/fixture"},
            "executionTimeoutSeconds":null,"model":null,"effort":null}},
        "content":{"revisionId":durable_steps::Uuid::now_v7().to_string(),
            "text":"retained 雪, quote \", slash \\ and\nline"},
        "watermarkLocalMs":1793440800000_i64,"firstDueMs":1793441100000_i64,
        "admittedThroughMs":1793440800000_i64,"externalOccupant":null,
        "carriedWaitingDueMs":null,"carriedFiring":null
    })
}

pub(super) fn required_value<'value>(
    root: &'value Value,
    pointer: &str,
) -> ConformanceResult<&'value Value> {
    root.pointer(pointer)
        .ok_or_else(|| format!("required handover fixture field missing: {pointer}").into())
}

pub(super) fn required_value_mut<'value>(
    root: &'value mut Value,
    pointer: &str,
) -> ConformanceResult<&'value mut Value> {
    root.pointer_mut(pointer)
        .ok_or_else(|| format!("required handover fixture field missing: {pointer}").into())
}
