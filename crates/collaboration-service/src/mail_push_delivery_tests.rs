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
    let push_id = id.as_uuid().to_string().try_into().unwrap();
    let push = runtime
        .fixture
        .owner
        .get_push_record(&push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(push.delivery_state, PushDeliveryState::Rejected);
    assert!(matches!(
        push.last_outcome.unwrap().outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: false,
            ..
        }
    ));
    assert_eq!(runtime.client.calls(id), 0);
    drop(mail);
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
