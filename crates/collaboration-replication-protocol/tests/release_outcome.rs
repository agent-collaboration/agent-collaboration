#![allow(clippy::expect_used)]

use collaboration_replication_protocol::{MoveResult, SessionRequest};
use serde_json::json;

#[test]
fn release_carries_either_final_position_or_migration_refusal() {
    for outcome in [
        json!({"kind":"final","value":{"epoch":1,"commitId":7}}),
        json!({"kind":"refused","value":{"kind":"migrationPending","migration":2,"state":"pending"}}),
        json!({"kind":"refused","value":{"kind":"migrationPending","migration":2,"state":"failed"}}),
    ] {
        let frame = json!({"kind":"release","value":{
            "project":"019f0000-0000-7000-8000-000000000001", "epoch":1,
            "transfer":"019f0000-0000-7000-8000-000000000002", "outcome":outcome
        }});
        let decoded: SessionRequest =
            serde_json::from_value(frame.clone()).expect("release outcome");
        assert_eq!(serde_json::to_value(decoded).expect("encoded"), frame);
    }
}

#[test]
fn move_can_report_pending_until_release_completes() {
    let pending = json!({"kind":"pending","transfer_id":"019f0000-0000-7000-8000-000000000002"});
    let result: MoveResult = serde_json::from_value(pending.clone()).expect("pending move");
    assert_eq!(serde_json::to_value(result).expect("encoded"), pending);
}

#[test]
fn holder_cannot_report_a_schema_refusal_owned_by_central_fence() {
    let outcome = serde_json::json!({"kind":"refused","value":{"kind":"schemaTooNew","store":{"version":1,"fingerprint":"schema"},"supported":0}});
    assert!(
        serde_json::from_value::<collaboration_replication_protocol::ReleaseOutcome>(outcome)
            .is_err()
    );
}
