//! A remote route survives restart without becoming a local timer or extending its deadline.
use automation_storage::{AutomationStore, RemoteWakeMail, WakeCreate};
use collaboration_protocol::{OperationId, SavedMessage, WakeSnapshot};
use serde_json::json;

#[allow(clippy::unwrap_used)] // Fixed fixture literal, independent of storage.
fn snapshot(id: &str, created: &str) -> WakeSnapshot {
    serde_json::from_value(json!({
        "definition":{"wakeupId":id,"changeId":"019f0000-0000-7000-8000-000000000009","createdBy":{"kind":"owner"},
        "message":{"target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"codex-local"},"sessionId":"recipient"},"content":{"kind":"humanUser","text":"saved"},"delivery":"auto"},
        "timing":{"kind":"after","seconds":60},"anchorAt":created,"expiresAt":"1970-01-01T00:05:00.000Z","createdAt":created},
        "state":"active","nextDueAt":"1970-01-01T00:01:00.000Z","firstFire":null,"pendingDeliveryId":null,"latestEventCursor":"owning cursor"
    })).unwrap()
}
#[allow(clippy::unwrap_used)] // Fixed closed request shape.
fn request(snapshot: &WakeSnapshot) -> RemoteWakeMail {
    let operation_id = OperationId::generate();
    RemoteWakeMail{
        operation_id:operation_id.clone(),target:snapshot.definition.message.target.clone(),canonical_request:vec![1,2,3],
        body:serde_json::from_value(json!({"requestedBy":{"kind":"owner"},"request":{"kind":"send","operationId":operation_id,"replace":false,"message":snapshot.definition.message,"timing":{"kind":"after","seconds":60},"expiry":{"kind":"none"}}})).unwrap(),valid_until_ms:10_000,
    }
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions are the regression oracle; Result propagates setup errors.
async fn route_reopen_replays_frozen_bytes_and_combined_pages_keep_membership()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let remote = snapshot(
        "019f0000-0000-7000-8000-000000000001",
        "1970-01-01T00:00:02.000Z",
    );
    let proposed = request(&remote);
    let operation = proposed.operation_id.clone();
    store.prepare_remote_wake(proposed.clone()).await?;
    store.remember_remote_wake(&operation, &remote).await?;
    store.close().await?;
    let mut store = AutomationStore::open(&path).await?;
    let mut repeated = proposed.clone();
    repeated.valid_until_ms = 50_000;
    let automation_storage::RemoteWakePreparation::Replayed {
        request: frozen,
        snapshot: retained,
    } = store.prepare_remote_wake(repeated).await?
    else {
        return Err("existing request was treated as new".into());
    };
    assert_eq!(
        serde_json::to_value(retained.ok_or("creation snapshot missing")?)?,
        serde_json::to_value(&remote)?
    );
    assert_eq!(frozen.valid_until_ms, 10_000);
    assert_eq!(frozen.body, proposed.body);
    assert_eq!(
        store
            .remote_wake_target(&remote.definition.wakeup_id)
            .await?,
        Some(remote.definition.message.target.clone())
    );
    assert!(matches!(
        store
            .read_wakeup::<SavedMessage>(&remote.definition.wakeup_id)
            .await,
        Err(automation_storage::StorageError::WakeNotFound)
    ));
    let mut conflicting = proposed;
    conflicting.canonical_request.push(4);
    assert!(matches!(
        store.prepare_remote_wake(conflicting).await,
        Err(automation_storage::StorageError::OperationConflict)
    ));
    let conflict = store
        .create_wakeup(&WakeCreate {
            operation_id: operation.clone(),
            requested_by: collaboration_protocol::WakeCreator::Owner {},
            replace: false,
            message: remote.definition.message.clone(),
            timing: agent_automation::TimingRule::After { seconds: 60 },
            expiry: agent_automation::ExpiryRule::None,
            now_ms: 1000,
        })
        .await;
    assert!(
        matches!(
            conflict,
            Err(automation_storage::StorageError::OperationConflict)
        ),
        "remote operation identity became a second local timer"
    );
    let local = store
        .create_wakeup(&WakeCreate {
            operation_id: OperationId::generate(),
            requested_by: collaboration_protocol::WakeCreator::Owner {},
            replace: false,
            message: remote.definition.message.clone(),
            timing: agent_automation::TimingRule::After { seconds: 60 },
            expiry: agent_automation::ExpiryRule::None,
            now_ms: 1000,
        })
        .await?;
    let first = store.wake_listing_keys(None, 1).await?;
    assert_eq!(first.records.len(), 1);
    assert_eq!(
        first
            .records
            .first()
            .ok_or("first record missing")?
            .wakeup_id,
        local.definition.wakeup_id
    );
    assert!(!first.records.first().ok_or("first record missing")?.remote);
    let newer = snapshot(
        "019f0000-0000-7000-8000-000000000002",
        "1970-01-01T00:00:03.000Z",
    );
    let newer_request = request(&newer);
    let newer_operation = newer_request.operation_id.clone();
    store.prepare_remote_wake(newer_request).await?;
    store.remember_remote_wake(&newer_operation, &newer).await?;
    let second = store.wake_listing_keys(first.next, 1).await?;
    assert_eq!(second.records.len(), 1);
    assert_eq!(
        second
            .records
            .first()
            .ok_or("second record missing")?
            .wakeup_id,
        remote.definition.wakeup_id
    );
    assert!(
        second
            .records
            .first()
            .ok_or("second record missing")?
            .remote
    );
    assert!(second.next.is_none());
    let unbound = request(&newer);
    store.prepare_remote_wake(unbound.clone()).await?;
    assert_eq!(store.prune_operation_receipts(31 * 86_400_000, 1).await?, 1);
    let automation_storage::RemoteWakePreparation::Replayed {
        request: retained,
        snapshot: None,
    } = store.prepare_remote_wake(unbound.clone()).await?
    else {
        return Err("ordinary receipt pruning deleted remote replay evidence".into());
    };
    assert_eq!(retained.valid_until_ms, unbound.valid_until_ms);
    assert_eq!(retained.canonical_request, unbound.canonical_request);
    assert_eq!(
        store
            .remote_wake_target(&remote.definition.wakeup_id)
            .await?,
        Some(remote.definition.message.target.clone())
    );
    store.close().await?;
    Ok(())
}
