//! Owned-row validation and write-once semantics over the actual native store.
use super::fixture::{Fixture, recorded_at};
use collaboration_mail_protocol::TooLateAt;
use collaboration_protocol::CancelledAfterDelivery;
use durable_steps::{AtomicStore, StoreError};
use interaction_storage::record_cancelled_after_delivery_in_scope;

#[tokio::test]
async fn recipient_cancel_mark_keeps_its_first_id_and_clock_after_a_second_owner_update() {
    let fixture = Fixture::new().await;
    let draft = fixture.draft();
    let target = draft.push_id.clone();
    fixture.settle_with_peer_receipt(draft).await;
    let first = CancelledAfterDelivery {
        cancel: uuid::Uuid::now_v7().to_string().try_into().unwrap(),
        too_late_at: TooLateAt::Delivered,
        recorded_at: recorded_at(),
    };
    let second = CancelledAfterDelivery {
        cancel: uuid::Uuid::now_v7().to_string().try_into().unwrap(),
        recorded_at: recorded_at() + chrono::Duration::seconds(100),
        ..first.clone()
    };
    for mark in [first.clone(), second] {
        let target = target.clone();
        fixture
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    record_cancelled_after_delivery_in_scope(scope, &target, &mark)
                        .await
                        .map_err(StoreError::backend)
                })
            })
            .await
            .unwrap();
    }
    let fixture = fixture.reopen().await;
    assert_eq!(
        fixture
            .owner
            .get_push_record(&target)
            .await
            .unwrap()
            .unwrap()
            .cancelled_after_delivery,
        Some(first)
    );
    drop(fixture.close().await);
}

#[tokio::test]
async fn invalid_stored_cancel_marks_are_refused_instead_of_becoming_absent() {
    let fixture = Fixture::new().await;
    let draft = fixture.draft();
    let target = draft.push_id.clone();
    fixture.settle_with_peer_receipt(draft).await;
    // Fault only the application's owned column, never SDK custody or journal tables.
    for statement in [
        "UPDATE router_pushes SET cancelled_after_delivery_json='{'",
        "UPDATE router_pushes SET cancelled_after_delivery_json='[]'",
        "UPDATE router_pushes SET cancelled_after_delivery_json='null'",
        "UPDATE router_pushes SET cancelled_after_delivery_json='{}'",
        "UPDATE router_pushes SET cancelled_after_delivery_json='{\"cancel\":\"invalid\",\"tooLateAt\":\"delivered\",\"recordedAt\":\"2026-10-19T18:00:00Z\"}'",
        "UPDATE router_pushes SET cancelled_after_delivery_json='{\"cancel\":\"018f47d2-24d5-7a68-b9ec-6f759c39458f\",\"tooLateAt\":\"unknown\",\"recordedAt\":\"2026-10-19T18:00:00Z\"}'",
        "UPDATE router_pushes SET cancelled_after_delivery_json='{\"cancel\":\"018f47d2-24d5-7a68-b9ec-6f759c39458f\",\"tooLateAt\":\"delivered\",\"recordedAt\":\"not-a-time\"}'",
        "UPDATE router_pushes SET cancelled_after_delivery_json='{\"cancel\":\"018f47d2-24d5-7a68-b9ec-6f759c39458f\",\"tooLateAt\":\"delivered\",\"recordedAt\":\"2026-10-19T18:00:00Z\",\"extra\":true}'",
        "UPDATE router_pushes SET cancelled_after_delivery_json='{\"cancel\":\"018f47d2-24d5-7a68-b9ec-6f759c39458f\",\"tooLateAt\":\"uncertain\",\"recordedAt\":\"2026-10-19T18:00:00Z\"}'",
    ] {
        fixture.sql(statement).await;
        assert!(
            fixture.owner.get_push_record(&target).await.is_err(),
            "accepted invalid stored mark: {statement}"
        );
    }
    let fixture = fixture.reopen().await;
    assert!(
        fixture.owner.get_push_record(&target).await.is_err(),
        "cold read silently accepted a contextually invalid mark"
    );
    fixture
        .sql("UPDATE router_pushes SET cancelled_after_delivery_json=NULL")
        .await;
    assert!(
        fixture
            .owner
            .get_push_record(&target)
            .await
            .unwrap()
            .unwrap()
            .cancelled_after_delivery
            .is_none()
    );
    drop(fixture.close().await);
}
