use serde_json::{Value, json};

pub fn schedule_payload() -> Value {
    json!({
        "automationId":"019f0000-0000-7000-8000-000000000001",
        "revision":"019f0000-0000-7000-8000-000000000002",
        "creator":{"kind":"owner"},
        "header":{"anchorAt":"2026-10-31T10:00:00.000Z",
            "createdAt":"2026-10-31T09:00:00.000Z","updatedAt":"2026-10-31T10:00:00.000Z",
            "importedContinuity":null},
        "definition":{"kind":"schedule","value":{
            "instructionId":"019f0000-0000-7000-8000-000000000003",
            "timing":{"kind":"at","at":"2026-10-31T10:05:00Z"},"enabled":true,
            "destination":{"kind":"ownedThread","target":{
                "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000004","endpointId":"codex-local"},
                "sessionId":"import-target"},"cwd":"/fixture"},
            "executionTimeoutSeconds":null,"model":null,"effort":null}},
        "content":{"revisionId":"019f0000-0000-7000-8000-000000000005","text":"retained local instruction"},
        "watermarkLocalMs":1793440800000_i64,"firstDueMs":1793441100000_i64,
        "admittedThroughMs":1793440800000_i64,
        "externalOccupant":null,"carriedWaitingDueMs":null,"carriedFiring":null
    })
}
