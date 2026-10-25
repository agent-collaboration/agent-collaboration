use super::super::SubscriptionClock;
use super::super::subscription_push::target_session;
use super::owner_fixture::*;
use crate::service_identity::subscription_delivery::subscription_service::OwnerObservation;
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushActivityRange, PushActivitySnapshot, PushDeliveryState,
    PushHeaderFacts, PushId, PushKind, PushOrigin, PushRecordDraft, RouterOriginRef,
};
use message_board::{
    BatchId, FeedCursor, SubscriptionMode, SubscriptionScope, ThreadId,
    ThreadSubscriptionUnsubscribeRequest, WhenIdle,
};
use sqlx::Connection;
use std::time::Duration;

// SQLite works on background threads; its acknowledgement bound needs real time.
#[tokio::test]
async fn cleanup_write_retry_does_not_block_reader_reconcile() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(&fixture.root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
        .await;
    let position = fixture
        .post(&fixture.root, "Held before ending scope")
        .await;
    let held_id = copied_held_snapshot(&fixture, vec![(fixture.root.clone(), position)]).await;
    let runtime = fixture.runtime().await;
    runtime.reconcile(&fixture.reader).await;
    runtime.owner_barrier(&fixture.reader).await;
    assert_copied_snapshot_state(&fixture, &held_id, PushDeliveryState::Held).await;
    while runtime.observations.lock().await.try_recv().is_ok() {}
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    sqlx::raw_sql("CREATE TRIGGER fail_held_cleanup BEFORE UPDATE OF delivery_state ON router_pushes WHEN OLD.delivery_state='held' AND NEW.delivery_state='rejected' BEGIN SELECT RAISE(FAIL, 'injected cleanup write failure'); END").execute(&mut observer).await.unwrap();
    fixture
        .unsubscribe(
            ThreadSubscriptionUnsubscribeRequest {
                reader: fixture.reader.clone(),
                scope: SubscriptionScope::thread(fixture.root.clone()),
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    runtime.reconcile(&fixture.reader).await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::Sleeping(Some(_))))
        .await;
    let acknowledged = tokio::time::timeout(
        Duration::from_secs(1),
        runtime.service.reconcile_reader(fixture.reader.clone()),
    )
    .await;
    assert!(
        matches!(acknowledged, Ok(Ok(()))),
        "cleanup write retry blocked a queued reconcile command"
    );
    runtime.owner_barrier(&fixture.reader).await;
    // A command acknowledges before the failed cleanup has been repaired.
    // The marker remains pending, and the next regular pass retries it.
    assert_eq!(
        fixture
            .interactions
            .get_push_record(&held_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        collaboration_protocol::PushDeliveryState::Held
    );
    sqlx::query("DROP TRIGGER fail_held_cleanup")
        .execute(&mut observer)
        .await
        .unwrap();
    fixture.clock.advance_without_tokio_time(30);
    tokio::time::timeout(
        Duration::from_secs(5),
        runtime
            .observe(|event| matches!(event, OwnerObservation::HeldSubscriptionPushesSettled(1))),
    )
    .await
    .expect("failed cleanup marker must survive until the repaired next pass");
    assert_copied_snapshot_state(&fixture, &held_id, PushDeliveryState::Rejected).await;
    let rejected = fixture
        .interactions
        .get_push_record(&held_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        rejected.last_outcome.unwrap().outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: false,
            reason: "subscription ended: cancelled".to_owned(),
        }
    );
    observer.close().await.unwrap();
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn ending_one_scope_preserves_a_held_batch_with_another_active_root() {
    let fixture = OwnerFixture::new().await;
    let active_root = fixture.add_root().await;
    for root in [&fixture.root, &active_root] {
        fixture
            .policy(root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
            .await;
    }
    let ending_position = fixture.post(&fixture.root, "Ending root").await;
    let active_position = fixture.post(&active_root, "Active root").await;
    let push_id = copied_held_snapshot(
        &fixture,
        vec![
            (fixture.root.clone(), ending_position),
            (active_root.clone(), active_position),
        ],
    )
    .await;
    let runtime = fixture.runtime().await;
    runtime.reconcile(&fixture.reader).await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture
        .unsubscribe(
            ThreadSubscriptionUnsubscribeRequest {
                reader: fixture.reader.clone(),
                scope: SubscriptionScope::thread(fixture.root.clone()),
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    runtime.reconcile(&fixture.reader).await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::HeldSubscriptionPushesSettled(_)))
        .await;
    assert_eq!(
        fixture
            .interactions
            .get_push_record(&push_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        collaboration_protocol::PushDeliveryState::Held,
        "one ended scope must not settle activity for a live root"
    );
    fixture
        .unsubscribe(
            ThreadSubscriptionUnsubscribeRequest {
                reader: fixture.reader.clone(),
                scope: SubscriptionScope::thread(active_root),
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    runtime.reconcile(&fixture.reader).await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::HeldSubscriptionPushesSettled(1)))
        .await;
    assert_copied_snapshot_state(&fixture, &push_id, PushDeliveryState::Rejected).await;
    runtime.close().await;
}

// Decision 756 maps the former live C5 Held cases to native_follow_mail_proofs
// (same-generation frozen retry) and native_follow_cancel_proofs (real membership
// end, frozen child, SDK Prevented and the canonical public cancellation reason).

// This is an explicit post-copy CreationAge snapshot. SDK admission and the D4 copy
// are outside these cleanup cases; public native writes establish only their input.
async fn copied_held_snapshot(
    fixture: &OwnerFixture,
    positions: Vec<(ThreadId, FeedCursor)>,
) -> PushId {
    let target = target_session(&fixture.reader).unwrap();
    let push_id = PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap();
    fixture
        .interactions
        .insert_push_record(PushRecordDraft {
            push_id: push_id.clone(),
            kind: PushKind::SubscriptionActivity,
            origin: PushOrigin::Router(PushKind::SubscriptionActivity),
            origin_router_ref: Some(
                RouterOriginRef::SubscriptionActivity {
                    target: target.clone(),
                    batch_id: BatchId::generate(),
                }
                .canonical_string()
                .unwrap(),
            ),
            target,
            reply_to_push_id: None,
            header_facts: PushHeaderFacts::SubscriptionActivity {
                root_count: u8::try_from(positions.len()).unwrap(),
                message_count: u64::try_from(positions.len()).unwrap(),
                held_since: Some(fixture.clock.now().to_rfc3339()),
                thread_resolved: false,
            },
            body: None,
            activity: Some(PushActivitySnapshot {
                ranges: positions
                    .into_iter()
                    .map(|(thread_id, position)| PushActivityRange {
                        thread_id,
                        project_id: fixture.project.clone(),
                        from: position,
                        through: position,
                    })
                    .collect(),
                held: true,
                draining: false,
            }),
            mode: None,
            guard: None,
            created_at: fixture.clock.now(),
        })
        .await
        .unwrap();
    fixture
        .interactions
        .mark_push_attempted(&push_id)
        .await
        .unwrap();
    fixture
        .interactions
        .hold_push_record(
            &push_id,
            DeliveryReceipt {
                outcome: DeliveryOutcome::NotSubmitted {
                    retryable: true,
                    reason: "copied held snapshot".to_owned(),
                },
                reachability: None,
                client: None,
            },
        )
        .await
        .unwrap();
    assert_copied_snapshot_state(fixture, &push_id, PushDeliveryState::Held).await;
    push_id
}

async fn assert_copied_snapshot_state(
    fixture: &OwnerFixture,
    push_id: &PushId,
    expected: PushDeliveryState,
) {
    let record = fixture
        .interactions
        .get_push_record(push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.delivery_state, expected);
    if expected == PushDeliveryState::Held {
        assert!(
            fixture
                .interactions
                .list_held_carrier_push_records(&record.target)
                .await
                .unwrap()
                .iter()
                .any(|candidate| candidate.push_id == *push_id),
            "fixture must use the CreationAge carrier class"
        );
    }
}
