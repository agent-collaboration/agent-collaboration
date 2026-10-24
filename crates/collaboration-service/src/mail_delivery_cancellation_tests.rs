//! Real Local control custody projects only cancellation metadata onto terminal mail.
use super::fixture::{Fixture, ProjectionTask, recorded_at};
use crate::RouterDeliveryTypes;
use collaboration_mail_protocol::CarrierAcceptance;
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushRecordDraft, SessionReachability,
};
use durable_steps::{AtomicStore, JournalScope, StoreError};
use fleet_sync_delivery::{Inbox, LocalCancelAdmission, LocalOutcome, delivery_rollback};
use fleet_sync_protocol::{CancelBody, DeliveryCancel, ItemId};
use runtime_primitives::SyncTime;
use serde_json::json;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn late_local_delivered_cancel_adds_metadata_and_preserves_the_original_result() {
    assert_terminal_local_cancel(
        LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::PeerMessageWritten,
        },
        DeliveryReceipt {
            outcome: DeliveryOutcome::PeerMessageWritten,
            reachability: Some(SessionReachability::ClaudeCodePeer),
            client: None,
        },
        "delivered",
    )
    .await;
}

#[tokio::test]
async fn late_local_uncertain_cancel_adds_metadata_without_another_native_attempt() {
    assert_terminal_local_cancel(
        LocalOutcome::Uncertain,
        DeliveryReceipt {
            outcome: DeliveryOutcome::Unknown,
            reachability: Some(SessionReachability::ClaudeCodePeer),
            client: None,
        },
        "uncertain",
    )
    .await;
}

#[tokio::test]
async fn local_cancel_commits_after_its_target_push_is_pruned_without_recreating_it() {
    let fixture = Fixture::new().await;
    let draft = fixture.draft();
    let push_id = draft.push_id.clone();
    let item = fixture.settle_with_peer_receipt(draft.clone()).await;
    let answer_id = item.as_uuid().to_string().try_into().unwrap();
    let answer = serde_json::to_value(
        fixture
            .owner
            .message_send_answer(&answer_id)
            .await
            .unwrap()
            .unwrap()
            .result
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        fixture
            .owner
            .prune_push_records(recorded_at() + chrono::Duration::days(31), 10)
            .await
            .unwrap(),
        1
    );
    let control = ItemId::new(uuid::Uuid::now_v7()).unwrap();

    let admitted = admit_cancel(&fixture, cancel_for(&fixture, &draft, control))
        .await
        .unwrap();

    assert_eq!(admitted.reference.item, control);
    assert!(admitted.effects.attempts.is_empty());
    assert!(admitted.effects.reports.is_empty());
    assert!(
        fixture
            .owner
            .get_push_record(&push_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        serde_json::to_value(
            fixture
                .owner
                .message_send_answer(&answer_id)
                .await
                .unwrap()
                .unwrap()
                .result
                .unwrap()
        )
        .unwrap(),
        answer
    );
    assert_eq!(fixture.host.calls.load(Ordering::SeqCst), 1);
    let fixture = fixture.reopen().await;
    let replay = admit_cancel(&fixture, cancel_for(&fixture, &draft, control))
        .await
        .unwrap();
    assert_eq!(replay.reference.item, control);
    assert!(
        fixture
            .owner
            .get_push_record(&push_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(fixture.host.calls.load(Ordering::SeqCst), 0);
    drop(fixture.close().await);
}

#[tokio::test]
async fn failed_local_cancel_projection_rolls_back_custody_and_retries_the_same_control() {
    let fixture = Fixture::new().await;
    let draft = fixture.draft();
    let push_id = draft.push_id.clone();
    let item = fixture.settle_with_peer_receipt(draft.clone()).await;
    let answer_id = item.as_uuid().to_string().try_into().unwrap();
    let before = serde_json::to_value(
        fixture
            .owner
            .get_push_record(&push_id)
            .await
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let answer = serde_json::to_value(
        fixture
            .owner
            .message_send_answer(&answer_id)
            .await
            .unwrap()
            .unwrap()
            .result
            .unwrap(),
    )
    .unwrap();
    fixture.sql("CREATE TRIGGER reject_cancel_projection BEFORE UPDATE OF cancelled_after_delivery_json ON router_pushes BEGIN SELECT RAISE(ABORT, 'fixture cancellation projection refused'); END").await;
    let control = ItemId::new(uuid::Uuid::now_v7()).unwrap();

    let error = admit_cancel(&fixture, cancel_for(&fixture, &draft, control))
        .await
        .err()
        .expect("the actual projection write must fail");

    assert!(
        error
            .to_string()
            .contains("fixture cancellation projection refused"),
        "{error:?}"
    );
    assert_eq!(
        serde_json::to_value(
            fixture
                .owner
                .get_push_record(&push_id)
                .await
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        before
    );
    assert_eq!(
        serde_json::to_value(
            fixture
                .owner
                .message_send_answer(&answer_id)
                .await
                .unwrap()
                .unwrap()
                .result
                .unwrap()
        )
        .unwrap(),
        answer
    );
    fixture.sql("DROP TRIGGER reject_cancel_projection").await;
    let fixture = fixture.reopen().await;
    // A wrongly committed control would replay without invoking the failed hook.
    let retried = admit_cancel(&fixture, cancel_for(&fixture, &draft, control))
        .await
        .unwrap();
    assert_eq!(retried.reference.item, control);
    assert_eq!(
        serde_json::to_value(
            fixture
                .owner
                .get_push_record(&push_id)
                .await
                .unwrap()
                .unwrap()
        )
        .unwrap()
        .get("cancelledAfterDelivery"),
        Some(&json!({
            "cancel":control,"tooLateAt":"delivered","recordedAt":"2026-10-19T18:00:00Z",
        }))
    );
    assert_eq!(
        serde_json::to_value(
            fixture
                .owner
                .message_send_answer(&answer_id)
                .await
                .unwrap()
                .unwrap()
                .result
                .unwrap()
        )
        .unwrap(),
        answer
    );
    assert_eq!(fixture.host.calls.load(Ordering::SeqCst), 0);
    drop(fixture.close().await);
}

async fn assert_terminal_local_cancel(
    outcome: LocalOutcome<CarrierAcceptance>,
    receipt: DeliveryReceipt,
    expected_too_late_at: &str,
) {
    // Arrange: a real journal attempt has settled through the board's ordinary hook.
    let fixture = Fixture::new().await;
    let draft = fixture.draft();
    let push_id = draft.push_id.clone();
    let item_id = fixture.item(&draft).id();
    assert!(fixture.admit_local(draft.clone()).await.unwrap());
    fixture.push_answer(outcome, receipt);
    let engine = fixture.engine();
    assert_eq!(engine.drive_due().await.unwrap().completed.len(), 1);
    drop(engine);
    let before = serde_json::to_value(
        fixture
            .owner
            .get_push_record(&push_id)
            .await
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let answer_id = item_id.as_uuid().to_string().try_into().unwrap();
    let answer = serde_json::to_value(
        fixture
            .owner
            .message_send_answer(&answer_id)
            .await
            .unwrap()
            .unwrap()
            .result
            .unwrap(),
    )
    .unwrap();
    let control = ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let cancel = cancel_for(&fixture, &draft, control);
    let cancel_digest = cancel.digest().clone();
    let clock = recorded_at() + chrono::Duration::seconds(100);
    fixture
        .host
        .clock
        .store(clock.timestamp_millis(), Ordering::SeqCst);

    // Act: the public Local cancel intake must run the actual SDK outcome hook.
    let admitted = admit_cancel(&fixture, cancel).await.unwrap();
    assert_eq!(admitted.reference.item, control);
    assert_eq!(admitted.reference.digest, cancel_digest);
    assert!(admitted.effects.attempts.is_empty());
    assert!(admitted.effects.reports.is_empty());

    // Assert: only the new mark changes; the full receipt and first settlement stay.
    let mut after = serde_json::to_value(
        fixture
            .owner
            .get_push_record(&push_id)
            .await
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let expected = json!({
        "cancel":control,
        "tooLateAt":expected_too_late_at,
        "recordedAt":"2026-10-19T18:01:40Z",
    });
    assert_eq!(
        after.get("cancelledAfterDelivery"),
        Some(&expected),
        "the target must show its actual Local control outcome"
    );
    after
        .as_object_mut()
        .unwrap()
        .remove("cancelledAfterDelivery");
    assert_eq!(
        after, before,
        "cancellation rewrote the original push or receipt"
    );
    assert_eq!(
        serde_json::to_value(
            fixture
                .owner
                .message_send_answer(&answer_id)
                .await
                .unwrap()
                .unwrap()
                .result
                .unwrap()
        )
        .unwrap(),
        answer
    );
    assert_eq!(fixture.host.calls.load(Ordering::SeqCst), 1);
    assert!(
        fixture.report(control).await.is_none(),
        "Local controls do not fabricate remote reports"
    );

    // A new proposed ID must keep the SDK's first canonical cancel and first mark.
    fixture.host.clock.fetch_add(100_000, Ordering::SeqCst);
    let second = admit_cancel(
        &fixture,
        cancel_for(&fixture, &draft, ItemId::new(uuid::Uuid::now_v7()).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(second.reference.item, control);
    assert_eq!(second.reference.digest, cancel_digest);
    assert!(second.effects.attempts.is_empty());
    assert!(second.effects.reports.is_empty());
    assert_eq!(
        serde_json::to_value(
            fixture
                .owner
                .get_push_record(&push_id)
                .await
                .unwrap()
                .unwrap()
        )
        .unwrap()
        .get("cancelledAfterDelivery"),
        Some(&expected)
    );
    assert_eq!(fixture.host.calls.load(Ordering::SeqCst), 1);

    let fixture = fixture.reopen().await;
    assert_eq!(
        serde_json::to_value(
            fixture
                .owner
                .get_push_record(&push_id)
                .await
                .unwrap()
                .unwrap()
        )
        .unwrap()
        .get("cancelledAfterDelivery"),
        Some(&expected)
    );
    assert_eq!(
        serde_json::to_value(
            fixture
                .owner
                .message_send_answer(&answer_id)
                .await
                .unwrap()
                .unwrap()
                .result
                .unwrap()
        )
        .unwrap(),
        answer
    );
    let replay = admit_cancel(&fixture, cancel_for(&fixture, &draft, control))
        .await
        .unwrap();
    assert_eq!(replay.reference.item, control);
    assert!(replay.effects.attempts.is_empty());
    assert!(replay.effects.reports.is_empty());
    assert_eq!(
        fixture.host.calls.load(Ordering::SeqCst),
        0,
        "cold replay called the carrier"
    );
    drop(fixture.close().await);
}

fn cancel_for(
    fixture: &Fixture,
    draft: &PushRecordDraft,
    control: ItemId,
) -> DeliveryCancel<RouterDeliveryTypes> {
    let original = fixture.item(draft);
    DeliveryCancel::new(
        control,
        original.sender().clone(),
        original.recipient().clone(),
        None,
        CancelBody {
            target: original.id(),
        },
    )
    .unwrap()
}

async fn admit_cancel(
    fixture: &Fixture,
    cancel: DeliveryCancel<RouterDeliveryTypes>,
) -> Result<LocalCancelAdmission, StoreError> {
    let config = fixture.host.config.local;
    let consumer = fixture.host.consumer.clone();
    let now = SyncTime::from_unix_millis(fixture.host.clock.load(Ordering::SeqCst));
    fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let admitted =
                    Inbox::<_, RouterDeliveryTypes>::new(scope, config.owner, config.limits)
                        .enqueue_local_cancel(cancel, consumer.as_ref(), now)
                        .await
                        .map_err(delivery_rollback)?;
                for attempt in &admitted.effects.attempts {
                    let key =
                        fleet_sync_delivery::delivery_request_key(&(attempt.item, attempt.attempt))
                            .map_err(delivery_rollback)?;
                    scope
                        .admit::<ProjectionTask>(&key, attempt)
                        .await
                        .map_err(StoreError::backend)?;
                }
                Ok(admitted)
            })
        })
        .await
}
