//! Late replay reads actual SDK custody; the owning automation store supplies the answer.
//! No session is attached: observing an old answer must not need a new remote dispatch.
use super::native::Runtime;
use crate::MailDeliveryRuntime;
use crate::collaboration_application::WakeOperations;
use automation_storage::RemoteWakeMail;
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_mail_protocol::{CarrierAcceptance, RemoteWakeResult, WakeBody};
use collaboration_protocol::{
    LocalMutationEvidence, LocalMutationState, OperationId, WakeMutationRequest, WakeSendRequest,
    WakeSnapshot, WakeState,
};
use durable_steps::{AtomicStore, StoreError};
use fleet_sync_delivery::{Outbox, delivery_rollback};
use fleet_sync_protocol::{
    DeliveryItem, DeliveryStatus, ItemId, PeerId, StatusPublication, StatusVersion,
};
use serde_json::json;
use std::sync::Arc;

fn service(last: u8) -> collaboration_protocol::UuidIdentity {
    format!("00000000-0000-4000-8000-{last:012}")
        .try_into()
        .unwrap()
}
fn create_request(deadline: chrono::DateTime<chrono::Utc>) -> WakeSendRequest {
    serde_json::from_value(json!({
        "operationId":OperationId::generate(),"requestedBy":{"kind":"owner"},"replace":false,
        "message":{"target":{"endpoint":{"serviceId":service(2),"endpointId":"codex-local"},"sessionId":"late-replay"},"content":{"kind":"humanUser","text":"retained remote wake"},"delivery":"auto"},
        "timing":{"kind":"at","at":(chrono::Utc::now()+chrono::Duration::seconds(60)).to_rfc3339_opts(chrono::SecondsFormat::Millis,true)},
        "expiry":{"kind":"at","at":deadline.to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
    })).unwrap()
}
fn creation_mail(
    request: &WakeSendRequest,
    deadline: chrono::DateTime<chrono::Utc>,
) -> RemoteWakeMail {
    RemoteWakeMail {
        operation_id: request.operation_id.clone(),
        target: request.message.target.clone(),
        canonical_request: serde_json::to_vec(request).unwrap(),
        body: serde_json::from_value(json!({"requestedBy":request.requested_by,"request":{
            "kind":"send","operationId":request.operation_id,"replace":request.replace,
            "message":request.message,"timing":request.timing,"expiry":request.expiry
        }}))
        .unwrap(),
        valid_until_ms: deadline.timestamp_millis(),
    }
}
fn pause_mail(
    request: &WakeMutationRequest,
    snapshot: &WakeSnapshot,
    deadline: chrono::DateTime<chrono::Utc>,
) -> RemoteWakeMail {
    let body: WakeBody = serde_json::from_value(json!({"requestedBy":{"kind":"owner"},"request":{
        "kind":"pause","operationId":request.operation_id,"wakeupId":request.wakeup_id
    }}))
    .unwrap();
    RemoteWakeMail {
        operation_id: request.operation_id.clone(),
        target: snapshot.definition.message.target.clone(),
        canonical_request: serde_json::to_vec(&(&snapshot.definition.message.target, &body))
            .unwrap(),
        body,
        valid_until_ms: deadline.timestamp_millis(),
    }
}
async fn freeze(runtime: &Runtime, mail: &RemoteWakeMail) {
    runtime
        .automation
        .lock()
        .await
        .prepare_remote_wake(mail.clone())
        .await
        .unwrap();
}
async fn publish_recorded_status(
    source: &Runtime,
    recipient: &Runtime,
    request: RemoteWakeMail,
    result: Option<RemoteWakeResult>,
) {
    let item = DeliveryItem::<RouterDeliveryTypes>::new(
        ItemId::new(uuid::Uuid::parse_str(request.operation_id.as_str()).unwrap()).unwrap(),
        collaboration_mail_protocol::MailSender::HumanUser {
            installation: String::from(service(1)).try_into().unwrap(),
        },
        fleet_sync_protocol::Recipient {
            peer: recipient.fixture.host.config.local.owner,
            address: serde_json::from_value(serde_json::to_value(request.target).unwrap()).unwrap(),
        },
        Some(runtime_primitives::SyncTime::from_unix_millis(
            request.valid_until_ms,
        )),
        board_sync_protocol::BoardDeliveryBody::Wake(request.body),
    )
    .unwrap();
    let id = item.id();
    let relay = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let limits = source.fixture.host.config.local.limits;
    source
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let mut outbox = Outbox::<_, RouterDeliveryTypes>::new(scope, limits);
                outbox
                    .enqueue(item, relay)
                    .await
                    .map_err(delivery_rollback)?;
                outbox
                    .publish_status(StatusPublication {
                        item: id,
                        version: StatusVersion::new(1),
                        cancel: None,
                        status: match result {
                            Some(result) => DeliveryStatus::Delivered {
                                acceptance: CarrierAcceptance::Wake(Box::new(result)),
                            },
                            None => DeliveryStatus::Uncertain,
                        },
                    })
                    .await
                    .map_err(delivery_rollback)?;
                Ok::<_, StoreError>(())
            })
        })
        .await
        .unwrap();
}
async fn after_deadline(runtime: &Runtime, deadline: chrono::DateTime<chrono::Utc>) {
    // The mail owner already reads this clock when deciding whether frozen
    // custody has expired. Advance that boundary without waiting real time.
    runtime.fixture.host.clock.store(
        (deadline + chrono::Duration::milliseconds(20)).timestamp_millis(),
        std::sync::atomic::Ordering::SeqCst,
    );
}
fn mail_runtime(source: &Runtime) -> Arc<MailDeliveryRuntime> {
    Arc::new(source.local_mail_runtime())
}

async fn assert_late_creation_replay(remember_creation: bool) {
    // Arrange a real owning mutation and its canonical SDK result before the deadline.
    let source = Runtime::new().await;
    let owner = Runtime::new().await;
    let deadline = chrono::Utc::now() + chrono::Duration::seconds(2);
    let request = create_request(deadline);
    let frozen = creation_mail(&request, deadline);
    let owner_service = service(2);
    let snapshot = WakeOperations::new(&owner_service, Some(&owner.automation))
        .wake_send_local(request.clone())
        .await
        .unwrap();
    freeze(&source, &frozen).await;
    if remember_creation {
        source
            .automation
            .lock()
            .await
            .remember_remote_wake(&request.operation_id, &snapshot)
            .await
            .unwrap();
    }
    publish_recorded_status(
        &source,
        &owner,
        frozen,
        Some(serde_json::from_value(json!({"kind":"snapshot","value":snapshot})).unwrap()),
    )
    .await;
    after_deadline(&source, deadline).await;
    let mail = mail_runtime(&source);
    let sender_service = service(1);
    // Act: the identical public operation is retried after its immutable deadline.
    let replay = WakeOperations::new(&sender_service, Some(&source.automation))
        .with_remote(Some(&mail))
        .wake_send(request)
        .await;
    // Assert: the original answer, not today's expired projection or a new timer.
    assert_eq!(
        serde_json::to_value(replay.expect("late creation must replay")).unwrap(),
        serde_json::to_value(snapshot).unwrap()
    );
    drop(mail);
    source.close().await;
    owner.close().await;
}

#[tokio::test]
async fn wake_operations_remote_creation_late_replay_returns_recorded_result() {
    assert_late_creation_replay(true).await;
    // A crash after SDK settlement but before the route write still has its answer.
    assert_late_creation_replay(false).await;
}

#[tokio::test]
async fn wake_operations_remote_expired_new_request_has_no_custody() {
    let source = Runtime::new().await;
    let deadline = chrono::Utc::now() - chrono::Duration::seconds(1);
    let request = create_request(deadline);
    let id = ItemId::new(uuid::Uuid::parse_str(request.operation_id.as_str()).unwrap()).unwrap();
    after_deadline(&source, deadline).await;
    let mail = mail_runtime(&source);
    let sender_service = service(1);
    let error = WakeOperations::new(&sender_service, Some(&source.automation))
        .with_remote(Some(&mail))
        .wake_send(request)
        .await
        .expect_err("a new expired request must not dispatch");
    assert!(matches!(
        error.reason,
        collaboration_protocol::WakeFailureReason::DeadlineExpired
    ));
    assert!(matches!(
        error.effects,
        LocalMutationEvidence::Local {
            mutation: LocalMutationState::None
        }
    ));
    let limits = source.fixture.host.config.local.limits;
    let state = source
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                Outbox::<_, RouterDeliveryTypes>::new(scope, limits)
                    .status(id)
                    .await
                    .map(|status| status.state)
                    .map_err(delivery_rollback)
            })
        })
        .await
        .unwrap();
    assert!(matches!(
        state,
        fleet_sync_delivery::OutboxStatusState::Unknown
    ));
    drop(mail);
    source.close().await;
}

async fn assert_late_pause_replay(retain_result: bool, retention_age: Option<chrono::Duration>) {
    let source = Runtime::new().await;
    let owner = Runtime::new().await;
    let creation_deadline = chrono::Utc::now() + chrono::Duration::seconds(120);
    let creation = create_request(creation_deadline);
    let owner_service = service(2);
    let owner_ops = WakeOperations::new(&owner_service, Some(&owner.automation));
    let snapshot = owner_ops.wake_send_local(creation.clone()).await.unwrap();
    freeze(&source, &creation_mail(&creation, creation_deadline)).await;
    source
        .automation
        .lock()
        .await
        .remember_remote_wake(&creation.operation_id, &snapshot)
        .await
        .unwrap();
    let request = WakeMutationRequest {
        operation_id: OperationId::generate(),
        wakeup_id: snapshot.definition.wakeup_id.clone(),
    };
    // An already frozen control is close to its deadline when this process resumes.
    let deadline = chrono::Utc::now() + chrono::Duration::seconds(1);
    let frozen = pause_mail(&request, &snapshot, deadline);
    freeze(&source, &frozen).await;
    let paused = owner_ops.wake_pause_local(request.clone()).await.unwrap();
    drop(owner_ops);
    assert_eq!(paused.wakeup.state, WakeState::Paused);
    let result = retain_result
        .then(|| serde_json::from_value(json!({"kind":"mutation","value":paused})).unwrap());
    publish_recorded_status(&source, &owner, frozen.clone(), result).await;
    after_deadline(&source, deadline).await;
    if let Some(age) = retention_age {
        collaboration_runtime::AutomationRetentionWorker::new(source.automation.clone())
            .prune_batch((deadline + age).timestamp_millis())
            .await
            .unwrap();
        let retained = source
            .automation
            .lock()
            .await
            .prepare_remote_wake(frozen)
            .await
            .unwrap();
        assert!(
            matches!(
                retained,
                automation_storage::RemoteWakePreparation::Replayed { snapshot: None, .. }
            ),
            "age-based retention deleted the unbound control's frozen replay evidence"
        );
    }
    let mail = mail_runtime(&source);
    let sender_service = service(1);
    let replay = WakeOperations::new(&sender_service, Some(&source.automation))
        .with_remote(Some(&mail))
        .wake_pause(request)
        .await;
    if retain_result {
        assert_eq!(
            serde_json::to_value(replay.expect("late pause must replay")).unwrap(),
            serde_json::to_value(paused).unwrap()
        );
    } else {
        let error = replay.expect_err("missing owning result must remain uncertain");
        assert!(
            matches!(
                error.effects,
                LocalMutationEvidence::Local {
                    mutation: LocalMutationState::Unknown
                }
            ),
            "lost pause answer falsely reports no mutation: {error:?}"
        );
    }
    drop(mail);
    source.close().await;
    owner.close().await;
}
#[tokio::test]
async fn wake_operations_remote_pause_late_replay_returns_recorded_result() {
    assert_late_pause_replay(true, None).await;
}
#[tokio::test]
async fn wake_operations_remote_lost_result_after_deadline_remains_unknown() {
    assert_late_pause_replay(false, None).await;
}

#[tokio::test]
async fn wake_operations_remote_aged_control_keeps_replay_after_retention() {
    // C36 retains identity evidence by retirement, rather than its history age.
    assert_late_pause_replay(true, Some(chrono::Duration::days(32))).await;
    assert_late_pause_replay(false, Some(chrono::Duration::days(32))).await;
}
