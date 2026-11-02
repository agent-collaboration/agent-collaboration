//! Remote wake receipts must survive the same advertised schemas MCP clients validate.
use super::emitted_result_schema_tests::{
    advertised_output_validator, validated_structured_content,
};
use super::*;
use collaboration_protocol::{
    MessageHistoryEntry, MessageHistoryResult, MessageShowResult, RemoteMessageStatus,
    WakeMutationRequest, WakeSendRequest, WakeState,
};

#[tokio::test]
async fn remote_wake_history_and_show_schemas_accept_cancelled_nullable_snapshots() {
    // The retained result comes from real local storage and owner operations.
    let directory = tempfile::tempdir().expect("automation directory");
    let store = std::sync::Arc::new(tokio::sync::Mutex::new(
        automation_storage::AutomationStore::open(&directory.path().join("automation.sqlite"))
            .await
            .expect("automation store"),
    ));
    // Arrange retained pre-upgrade state, then read it through the actual public owner.
    let request: WakeSendRequest = serde_json::from_value(serde_json::json!({
        "operationId":OperationId::generate(),"requestedBy":{"kind":"owner"},"replace":false,
        "message":{"target":fixture_session("codex-local","schema-wake"),"content":{"kind":"humanUser","text":"schema regression"},"delivery":"auto"},
        "timing":{"kind":"after","seconds":60},"expiry":{"kind":"after","seconds":120}
    })).expect("legacy wake request");
    let created_id = store
        .lock()
        .await
        .create_wakeup(&automation_storage::WakeCreate {
            operation_id: request.operation_id,
            requested_by: request.requested_by,
            replace: request.replace,
            message: request.message,
            timing: serde_json::from_value(serde_json::to_value(request.timing).expect("timing"))
                .expect("legacy timing"),
            expiry: serde_json::from_value(serde_json::to_value(request.expiry).expect("expiry"))
                .expect("legacy expiry"),
            now_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("fixture clock after epoch")
                .as_millis()
                .try_into()
                .expect("fixture milliseconds fit storage"),
        })
        .await
        .expect("actual legacy storage constructor")
        .definition
        .wakeup_id;
    let application = crate::api_test_harness::test_identity()
        .with_automation_store(store.clone())
        .build();
    let collaboration_service::WakeReadResponse::Local { value: created } = application
        .wakes()
        .wake_show(collaboration_protocol::WakeShowRequest {
            wakeup_id: created_id,
        })
        .await
        .expect("read retained Wake")
    else {
        panic!("retained local schema fixture changed owner");
    };
    let cancelled = application
        .wakes()
        .wake_cancel(WakeMutationRequest {
            operation_id: OperationId::generate(),
            wakeup_id: created.definition.wakeup_id.clone(),
        })
        .await
        .expect("cancel real wake");
    let collaboration_service::WakeMutationResponse::Local(cancelled) = cancelled else {
        panic!("retained local cancellation changed owner");
    };
    assert_eq!(cancelled.wakeup.state, WakeState::Cancelled);
    let cancelled_wire = serde_json::to_value(&cancelled.wakeup).expect("cancelled snapshot");
    for field in ["nextDueAt", "firstFire", "pendingDeliveryId"] {
        assert_eq!(cancelled_wire.get(field), Some(&Value::Null), "{field}");
    }
    assert!(
        cancelled_wire["definition"]["message"]
            .get("generationGuard")
            .is_none()
    );

    // Also cover the required nullable expiry contract (retained wakes may have no expiry).
    let mut no_expiry = cancelled_wire.clone();
    no_expiry["definition"]["expiresAt"] = Value::Null;
    for snapshot in [
        cancelled_wire,
        serde_json::to_value(&created).expect("active snapshot"),
        no_expiry,
    ] {
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
    drop(application);
    std::sync::Arc::try_unwrap(store)
        .unwrap_or_else(|_| panic!("schema fixture still owns storage"))
        .into_inner()
        .close()
        .await
        .expect("close schema fixture");
}
