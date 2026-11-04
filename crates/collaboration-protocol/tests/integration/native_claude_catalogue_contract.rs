//! The reviewed shared catalogue is a public read contract, not a peer disguise.
use collaboration_protocol::{ProviderSessionListResult, ProviderSessionSummary};
use serde_json::{Value, json};

fn background_summary() -> Value {
    json!({"origin":"claudeCodeBackground","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"00000000-0000-4000-8000-000000000042"},
        "jobHandle":"job-handle-42","workingDirectory":"/source/project","name":"Before first prompt","startedAt":1790162100_i64,"state":{"kind":"blocked"},"status":null,"peer":null})
}

#[test]
fn background_job_summary_keeps_native_uuid_separate_and_has_no_invented_updated_or_peer_fields() {
    let wire = background_summary();
    let summary: ProviderSessionSummary = serde_json::from_value(wire)
        .expect("job-only native observation must have its own public variant");
    assert_eq!(
        String::from(summary.target().session_id.clone()),
        "00000000-0000-4000-8000-000000000042"
    );
    let serialized = serde_json::to_value(summary).unwrap();
    assert_eq!(serialized["origin"], "claudeCodeBackground");
    assert_eq!(serialized["jobHandle"], "job-handle-42");
    assert_eq!(serialized["state"], json!({"kind":"blocked"}));
    assert!(serialized.get("updatedAt").is_none());
    assert!(serialized.get("pid").is_none());
    assert!(serialized.get("messagingSocketPath").is_none());
    assert!(serialized.get("requestedPolicy").is_none());
    assert!(serialized.get("peer").is_none_or(Value::is_null));
}

#[test]
fn native_coverage_preserves_peer_and_background_availability_independently() {
    for background in [
        json!({"kind":"complete"}),
        json!({"kind":"incomplete","unidentifiedJobs":2}),
        json!({"kind":"unavailable","reason":"readTimedOut"}),
    ] {
        let expected = json!({"peer":{"kind":"complete"},"background":background});
        let wire = json!({"endpoint":background_summary()["target"]["endpoint"],"observedAt":"2026-10-18T00:00:00Z","sessions":[background_summary()],"nextCursor":null,"nativeCoverage":expected});
        let page: ProviderSessionListResult = serde_json::from_value(wire)
            .expect("public native coverage cannot be silently discarded");
        assert_eq!(
            serde_json::to_value(page).unwrap()["nativeCoverage"],
            expected
        );
    }
}
