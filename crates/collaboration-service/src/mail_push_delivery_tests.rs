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

use native::*;

#[tokio::test]
async fn dispatch_drops_an_arriving_board_notification_without_native_delivery() {
    let runtime = Runtime::new().await;
    let notification = follow(&runtime, 2, 10);
    let id = notification.id();
    // The native client would accept the message if dispatch let it through.
    runtime.client.script(id, vec![accepted()]);
    runtime
        .fixture
        .receive_items(vec![notification])
        .await
        .unwrap();

    let mail = runtime.local_mail_runtime();
    mail.drive_due_for_test().await.unwrap();

    assert_eq!(
        runtime.fixture.local_outcome(id).await,
        Some(RecipientOutcome::Dropped)
    );
    assert_eq!(runtime.client.calls(id), 0);
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn startup_settles_a_stored_held_board_notification() {
    use collaboration_mail_protocol::{FollowDeliveryPolicy, NotificationSource};
    use fleet_sync_delivery::LocalOutcome;
    use message_board::{BatchTiming, Identity, SubscriptionLifetime, SubscriptionMode, WhenIdle};
    use tokio_util::sync::CancellationToken;

    let runtime = Runtime::new().await;
    let poll = follow(&runtime, 2, 10);
    let mut poll_body = poll.body().clone();
    let board_sync_protocol::BoardDeliveryBody::Notification(notification) = &mut poll_body else {
        panic!("expected a notification fixture");
    };
    let NotificationSource::FollowBatch { policy, .. } = &mut notification.source else {
        panic!("expected a follow fixture");
    };
    *policy = FollowDeliveryPolicy::new(
        &Identity::Session {
            session: poll.recipient().address.clone(),
        },
        SubscriptionMode::Poll,
        WhenIdle::Hold,
        BatchTiming::new(2, 10).unwrap(),
        SubscriptionLifetime::new(600).unwrap(),
    )
    .unwrap();
    let held = DeliveryItem::new(
        poll.id(),
        poll.sender().clone(),
        poll.recipient().clone(),
        None,
        poll_body,
    )
    .unwrap();
    let held_id = held.id();

    // The old reader returned Held without a deadline. Real SDK custody and its
    // pending poll projection persist that state across the owner reopen.
    runtime.fixture.host.consumer.native.lock().unwrap().take();
    runtime.fixture.receive_items(vec![held]).await.unwrap();
    runtime.fixture.push_answer(
        LocalOutcome::Held { until: None },
        DeliveryReceipt {
            outcome: DeliveryOutcome::NotSubmitted {
                retryable: true,
                reason: "no poll waiter".into(),
            },
            reachability: None,
            client: None,
        },
    );
    runtime.engine().drive_due().await.unwrap();
    assert!(matches!(
        runtime.fixture.local_outcome(held_id).await,
        Some(RecipientOutcome::Held)
    ));
    let runtime = runtime.reopen().await;
    let mail = runtime.local_mail_runtime();
    let shutdown = CancellationToken::new();
    let worker_stop = shutdown.clone();
    let worker = tokio::spawn(async move { mail.run(worker_stop).await });
    let outcomes = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !matches!(
            runtime.fixture.local_outcome(held_id).await,
            Some(RecipientOutcome::Dropped)
        ) {
            tokio::task::yield_now().await;
        }
    })
    .await;
    shutdown.cancel();
    worker.await.unwrap().unwrap();

    outcomes.expect("the held board notification became terminal");
    assert!(
        runtime
            .fixture
            .owner
            .pending_poll_item(held_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(runtime.client.calls(held_id), 0);
    runtime.close().await;
}

#[path = "collaboration_application/message_status_operations_test.rs"]
mod absent_status;
#[path = "mail_delivery_runtime_retention_test.rs"]
mod custody_retention;
#[path = "mail_push_direct_delivery_tests.rs"]
mod direct;
#[path = "inbound_wake_refusal_tests.rs"]
mod inbound_wake;
#[path = "local_mail_runtime_tests.rs"]
mod local_runtime;
#[path = "mail_materialized_session_test.rs"]
mod materialized_session;
#[path = "mail_removal_backfill_test.rs"]
mod removal_backfill;
#[path = "mail_submitter_backfill_test.rs"]
mod submitter_backfill;

#[tokio::test]
async fn retry_counter_failure_rolls_back_outcome_and_saved_native_answer_does_not_resend() {
    let runtime = Runtime::new().await;
    let item = runtime.fixture.item(&runtime.fixture.draft());
    let id = item.id();
    runtime.client.script_results(
        id,
        vec![
            collaboration_contracts::NativeDeliveryResult::not_submitted(
                "recipient unloaded during delivery",
                true,
                collaboration_contracts::UnsentCause::Holdable {
                    otherwise: collaboration_contracts::NativeRefusalCode::CarrierUnavailable,
                },
            ),
        ],
    );
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
    assert_eq!(runtime.count(id).await, 0);
    assert!(matches!(
        runtime.fixture.report(id).await,
        Some(RecipientOutcome::Held)
    ));
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
