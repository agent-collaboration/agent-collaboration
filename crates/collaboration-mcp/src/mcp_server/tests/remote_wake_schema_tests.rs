//! Remote wake receipts must survive the same advertised schemas MCP clients validate.
use super::emitted_result_schema_tests::{
    advertised_output_validator, validated_structured_content,
};
use super::*;
use collaboration_protocol::{
    MessageHistoryEntry, MessageHistoryResult, MessageShowResult, RemoteMessageStatus,
};

/// A cancelled wake snapshot as a remote Router reports it: no next due time, first fire or
/// pending delivery. Recorded from the wake operations before they were removed from this
/// build; the remote acceptance decodes it into the portable snapshot.
fn cancelled_wake_snapshot() -> Value {
    serde_json::json!({
        "definition": {
            "wakeupId": "01a1f5d7-0542-7523-b8d6-2e17bbc686f4",
            "changeId": "01a1f5d7-0542-7523-b8d6-2e2cc8d845a1",
            "createdBy": {"kind": "owner"},
            "message": {
                "target": fixture_session("codex-local", "schema-wake"),
                "content": {"kind": "humanUser", "text": "schema regression"},
                "delivery": "auto"
            },
            "timing": {"kind": "after", "seconds": 60},
            "anchorAt": "2026-11-19T22:04:31.936Z",
            "expiresAt": "2026-11-19T22:06:31.936Z",
            "createdAt": "2026-11-19T22:04:31.936Z"
        },
        "state": "cancelled",
        "nextDueAt": null,
        "firstFire": null,
        "pendingDeliveryId": null,
        "latestEventCursor": "[1,\"00000000-0000-4000-8000-000000000001\",\"automation-events\",2,1795125871938]"
    })
}

#[tokio::test]
async fn remote_wake_history_and_show_schemas_accept_cancelled_nullable_snapshots() {
    // Arrange: a cancelled snapshot, the active one it came from, and one with no expiry
    // (retained wakes may have none).
    let cancelled_wire = cancelled_wake_snapshot();
    let mut active = cancelled_wire.clone();
    active["state"] = serde_json::json!("active");
    active["nextDueAt"] = serde_json::json!("2026-11-19T22:05:31.936Z");
    let mut no_expiry = cancelled_wire.clone();
    no_expiry["definition"]["expiresAt"] = Value::Null;
    for snapshot in [cancelled_wire, active, no_expiry] {
        // Decoding here constructs the portable RemoteWakeSnapshot inside the real acceptance.
        let mail: RemoteMessageStatus = serde_json::from_value(serde_json::json!({
            "item": FIXTURE_PUSH_ID,
            "context": {
                "kind":"direct", "origin":"ownerUnverified",
                "recipient": fixture_session("codex-local", "schema-wake")
            },
            "recordedAt":"2026-10-23T22:00:00Z",
            "status": {"kind":"delivered", "acceptance":{"wake":{
                "kind":"snapshot", "value":snapshot
            }}}
        }))
        .expect("typed remote wake acceptance");
        let history = MessageHistoryResult {
            records: vec![MessageHistoryEntry::Remote { mail: mail.clone() }],
        };
        let shown = MessageShowResult::Remote {
            mail: Box::new(mail),
        };
        for (tool, result, snapshot_path) in [
            (
                "message_history",
                super::super::success_result(&history),
                "/records/0/mail/status/acceptance/wake/value",
            ),
            (
                "router_show",
                super::super::success_result(&shown),
                "/mail/status/acceptance/wake/value",
            ),
        ] {
            // Act: serialize the actual success payload and validate the generated catalog.
            let validator = advertised_output_validator(tool);
            let encoded = validated_structured_content(&validator, tool, result);
            assert_eq!(encoded.pointer(snapshot_path), Some(&snapshot));

            // Assert both directions: nullable does not mean missing or an arbitrary type.
            for (parent_path, field) in [
                (snapshot_path.to_owned(), "nextDueAt"),
                (snapshot_path.to_owned(), "firstFire"),
                (snapshot_path.to_owned(), "pendingDeliveryId"),
                (format!("{snapshot_path}/definition"), "expiresAt"),
            ] {
                let mut missing = encoded.clone();
                assert!(
                    missing
                        .pointer_mut(&parent_path)
                        .unwrap()
                        .as_object_mut()
                        .unwrap()
                        .remove(field)
                        .is_some()
                );
                assert!(!validator.is_valid(&missing), "{tool} must require {field}");
                let mut wrong_type = encoded.clone();
                wrong_type.pointer_mut(&parent_path).unwrap()[field] = serde_json::json!(42);
                assert!(!validator.is_valid(&wrong_type), "{tool} must type {field}");
            }
        }
    }
}
