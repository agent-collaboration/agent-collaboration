//! Real native actor and library retry boundaries with explicit external seams.
use super::*;
#[path = "mail_push_delivery_fixture.rs"]
mod native;
#[path = "mail_delivery_runner_probe.rs"]
mod runner_probe;
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, PushDeliveryState, PushId};
use durable_steps::{AtomicStore, StoreError};
use fleet_sync_delivery::Inbox;
use fleet_sync_protocol::RecipientOutcome;

#[tokio::test]
async fn poll_has_no_push_or_native_call_at_the_delivery_entry() {
    let runtime = Runtime::new().await;
    let carrier = runtime
        .fixture
        .host
        .consumer
        .native
        .lock()
        .unwrap()
        .clone()
        .unwrap();
    let call = runtime_primitives::CallId {
        operation: runtime_primitives::OperationId::new(uuid::Uuid::now_v7()).unwrap(),
        step: runtime_primitives::StepName::new("deliver").unwrap(),
    };
    let template = follow(&runtime, 2, 10);
    let mut body = template.body().clone();
    let board_sync_protocol::BoardDeliveryBody::Notification(notification) = &mut body else {
        panic!("notification");
    };
    let collaboration_mail_protocol::NotificationSource::FollowBatch { policy, .. } =
        &mut notification.source
    else {
        panic!("follow batch");
    };
    *policy = collaboration_mail_protocol::FollowDeliveryPolicy::new(
        &message_board::Identity::Session {
            session: template.recipient().address.clone(),
        },
        message_board::SubscriptionMode::Poll,
        message_board::WhenIdle::Hold,
        message_board::BatchTiming::new(2, 10).unwrap(),
        message_board::SubscriptionLifetime::new(600).unwrap(),
    )
    .unwrap();
    let poll = fleet_sync_protocol::DeliveryItem::new(
        template.id(),
        template.sender().clone(),
        template.recipient().clone(),
        None,
        body,
    )
    .unwrap();
    assert!(carrier.deliver_push(&poll, &call).await.unwrap().is_none());
    assert_eq!(runtime.client.calls(poll.id()), 0);
    assert!(
        runtime
            .fixture
            .owner
            .get_push_record(&poll.id().as_uuid().to_string().try_into().unwrap())
            .await
            .unwrap()
            .is_none()
    );
    drop(carrier);
    runtime.close().await;
}
use native::*;

#[path = "mail_push_direct_delivery_tests.rs"]
mod direct;
#[path = "local_mail_runtime_tests.rs"]
mod local_runtime;
#[path = "remote_mail_assembly_tests.rs"]
mod remote_assembly;
#[path = "remote_wake_replay_tests.rs"]
mod remote_wakes;

#[tokio::test]
async fn remote_follow_retries_use_item_counts_and_library_deadlines_across_cold_reopen() {
    // Arrange: same root and activation, disjoint items, and no sender window/project.
    let runtime = Runtime::new().await;
    let first = follow(&runtime, 2, 10);
    let first_id = first.id();
    let second = follow(&runtime, 20, 30);
    let second_id = second.id();
    runtime
        .client
        .script(first_id, vec![rejected(), rejected(), accepted()]);
    runtime.client.script(
        second_id,
        vec![
            DeliveryReceipt {
                outcome: DeliveryOutcome::NotSubmitted {
                    retryable: true,
                    reason: "not loaded".to_owned(),
                },
                reachability: None,
                client: None,
            },
            accepted(),
        ],
    );
    runtime
        .fixture
        .receive_items(vec![first, second])
        .await
        .unwrap();
    let engine = runtime.engine();
    engine.drive_due().await.unwrap();
    assert_eq!(runtime.count(first_id).await, 1);
    assert_eq!(runtime.count(second_id).await, 0);
    assert_eq!(
        runtime
            .sql_count("SELECT COUNT(*) AS count FROM subscription_delivery_progress")
            .await,
        0
    );
    let push: PushId = first_id.as_uuid().to_string().try_into().unwrap();
    let held = runtime
        .fixture
        .owner
        .get_push_record(&push)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.delivery_state, PushDeliveryState::Held);
    assert_eq!(
        serde_json::to_value(held.last_outcome).unwrap(),
        serde_json::to_value(Some(rejected())).unwrap()
    );
    assert!(matches!(
        runtime.fixture.report(first_id).await,
        Some(RecipientOutcome::Held)
    ));
    drop(engine);
    // Act: reopen the physical owner, then drive the real library task on both deadline edges.
    let runtime = runtime.reopen().await;
    assert_eq!(runtime.count(first_id).await, 1);
    let engine = runtime.engine();
    runtime.set_time(29_999);
    engine.drive_due().await.unwrap();
    assert_eq!(
        (
            runtime.client.calls(first_id),
            runtime.client.calls(second_id)
        ),
        (1, 1)
    );
    runtime.set_time(30_000);
    engine.drive_due().await.unwrap();
    assert_eq!(runtime.client.calls(first_id), 1);
    // The SDK's existing 10ms retry floor applies after the one-millisecond
    // early check. Read its durable schedule rather than assuming a timer.
    assert_eq!(
        runtime.next_attempt(first_id, 1).await,
        recorded_at().timestamp_millis() + 30_009
    );
    runtime.set_time(30_009);
    engine.drive_due().await.unwrap();
    assert_eq!(runtime.client.calls(first_id), 2);
    assert_eq!(runtime.count(first_id).await, 2);
    runtime.set_time(89_999);
    engine.drive_due().await.unwrap();
    assert_eq!(runtime.client.calls(first_id), 2);
    runtime.set_time(90_009);
    engine.drive_due().await.unwrap();
    assert_eq!(runtime.client.calls(first_id), 3);
    // The terminal item's counter is gone. The other Held item retains its
    // readiness row with zero attempts and no retry schedule.
    assert_eq!(
        runtime
            .sql_item_count(
                "SELECT COUNT(*) AS count FROM local_delivery_retries WHERE item_id=?",
                first_id,
            )
            .await,
        0
    );
    assert_eq!(
        runtime
            .sql_item_count(
                "SELECT COUNT(*) AS count FROM local_delivery_retries WHERE item_id=? AND retry_attempts=0 AND scheduled_retry_at IS NULL",
                second_id,
            )
            .await,
        1
    );
    assert_eq!(
        runtime.client.calls(second_id),
        1,
        "an indefinite hold must not acquire a deadline from the other item"
    );
    let config = runtime.fixture.host.config;
    runtime
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                Inbox::<_, RouterDeliveryTypes>::new(
                    scope,
                    config.local.owner,
                    config.local.limits,
                )
                .release_held::<ProjectionTask>(
                    second_id,
                    SyncTime::from_unix_millis(recorded_at().timestamp_millis() + 90_000),
                )
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await
        .unwrap();
    engine.drive_due().await.unwrap();
    // Assert: only the released item receives another effect, and terminal outcomes remove counters.
    assert_eq!(
        (
            runtime.client.calls(first_id),
            runtime.client.calls(second_id)
        ),
        (3, 2)
    );
    assert!(matches!(
        runtime.fixture.local_outcome(first_id).await,
        Some(RecipientOutcome::Delivered { .. })
    ));
    assert!(matches!(
        runtime.fixture.local_outcome(second_id).await,
        Some(RecipientOutcome::Delivered { .. })
    ));
    assert_eq!(
        runtime
            .sql_count("SELECT COUNT(*) AS count FROM local_delivery_retries")
            .await,
        0
    );
    drop(engine);
    runtime.close().await;
}

#[tokio::test]
async fn retry_counter_failure_rolls_back_outcome_and_saved_native_answer_does_not_resend() {
    let runtime = Runtime::new().await;
    let item = follow(&runtime, 2, 10);
    let id = item.id();
    runtime.client.script(id, vec![rejected(), accepted()]);
    runtime.fixture.receive_items(vec![item]).await.unwrap();
    runtime.fixture.sql("CREATE TRIGGER reject_retry_count BEFORE INSERT ON local_delivery_retries BEGIN SELECT RAISE(ABORT,'fixture retry counter refused'); END").await;
    let engine = runtime.engine();
    let report = engine.drive_due().await.unwrap();
    assert!(!report.retried.is_empty(), "{report:?}");
    assert_eq!(runtime.count(id).await, 0);
    assert_eq!(runtime.client.calls(id), 1);
    assert!(runtime.fixture.local_outcome(id).await.is_none());
    assert!(runtime.fixture.report(id).await.is_none());
    assert_eq!(
        runtime
            .fixture
            .owner
            .get_push_record(&id.as_uuid().to_string().try_into().unwrap())
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Attempted
    );
    runtime.fixture.sql("DROP TRIGGER reject_retry_count").await;
    runtime.set_time(100_000);
    engine.drive_due().await.unwrap();
    assert_eq!(
        runtime.client.calls(id),
        1,
        "saved native answer must finish projection without replaying the effect"
    );
    assert_eq!(runtime.count(id).await, 1);
    assert!(matches!(
        runtime.fixture.report(id).await,
        Some(RecipientOutcome::Held)
    ));
    engine.drive_due().await.unwrap();
    assert_eq!(runtime.client.calls(id), 2);
    assert!(matches!(
        runtime.fixture.local_outcome(id).await,
        Some(RecipientOutcome::Delivered { .. })
    ));
    assert_eq!(
        runtime
            .sql_count("SELECT COUNT(*) AS count FROM local_delivery_retries")
            .await,
        0
    );
    drop(engine);
    runtime.close().await;
}

#[tokio::test]
async fn retry_counts_validate_stored_ranges_and_saturate_without_changing_identity() {
    let fixture = Fixture::new().await;
    let item: PushId = "01900000-0000-7000-8000-000000000200"
        .to_owned()
        .try_into()
        .unwrap();
    assert_eq!(
        fixture
            .owner
            .local_delivery_retry_attempts(&item)
            .await
            .unwrap(),
        0
    );
    fixture.sql("INSERT INTO local_delivery_retries(item_id,retry_attempts) VALUES ('01900000-0000-7000-8000-000000000200',-1)").await;
    assert!(matches!(
        fixture.owner.local_delivery_retry_attempts(&item).await,
        Err(interaction_storage::StorageError::InvalidStoredRecord)
    ));
    fixture
        .sql("UPDATE local_delivery_retries SET retry_attempts=4294967296")
        .await;
    assert!(matches!(
        fixture.owner.local_delivery_retry_attempts(&item).await,
        Err(interaction_storage::StorageError::InvalidStoredRecord)
    ));
    fixture
        .sql("UPDATE local_delivery_retries SET retry_attempts=4294967295")
        .await;
    let key = item.clone();
    fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                interaction_storage::record_local_delivery_retry_in_scope(scope, &key, true, None)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await
        .unwrap();
    assert_eq!(
        fixture
            .owner
            .local_delivery_retry_attempts(&item)
            .await
            .unwrap(),
        u32::MAX
    );
    let fixture = fixture.reopen().await;
    assert_eq!(
        fixture
            .owner
            .local_delivery_retry_attempts(&item)
            .await
            .unwrap(),
        u32::MAX
    );
    drop(fixture.close().await);
}

#[tokio::test]
async fn preceding_composed_schema_migrates_and_mismatched_retry_ledger_refuses() {
    observation_schema::assert_historical_prefixes_and_exact_ledger().await;
}

#[path = "mail_observation_schema_tests.rs"]
mod observation_schema;
