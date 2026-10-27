//! Real SDK outcomes and native actors feed the public view; only the carrier answers stand in.
use super::super::owner_fixture::*;
use crate::{CollaborationApplication, ServiceIdentity, SubscriptionClock, TargetPresence};
use collaboration_protocol::{DeliveryOutcome, PushId, ThreadSubscriptionsRequest};
use interaction_storage::MailDeliveryObservation;
use message_board::SubscriptionDeliveryOutcome;
use sqlx::Connection;
use std::time::Duration;

async fn set_observation_time(fixture: &OwnerFixture, push: &PushId, value: &str) {
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query("UPDATE router_pushes SET last_outcome_at=? WHERE push_id=?")
            .bind(value)
            .bind(push.as_str())
            .execute(&mut observer)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    observer.close().await.unwrap();
}

fn application(fixture: &OwnerFixture, runtime: &OwnerRuntime) -> CollaborationApplication {
    CollaborationApplication::new(
        ServiceIdentity::new(SERVICE_ID, SERVICE_ID)
            .unwrap()
            .with_project_admission(fixture.admission.clone())
            .with_subscription_delivery_service(runtime.service.clone(), fixture.presence.clone()),
    )
}

async fn observed_reason(
    fixture: &OwnerFixture,
    push: &PushId,
    reason: &str,
) -> MailDeliveryObservation {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(observed) = fixture
                .interactions
                .mail_delivery_observation(push)
                .await
                .unwrap()
                && observed.last_outcome.as_ref().is_some_and(|receipt| {
                    matches!(&receipt.outcome,
                    DeliveryOutcome::NotSubmitted { reason: actual, .. } if actual == reason)
                })
            {
                break observed;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("actual SDK outcome committed")
}

async fn view(
    app: &CollaborationApplication,
    fixture: &OwnerFixture,
) -> collaboration_protocol::ThreadSubscriptionView {
    let mut rows = app
        .board()
        .thread_subscriptions(ThreadSubscriptionsRequest {
            actor: fixture.reader.clone(),
        })
        .await
        .unwrap()
        .subscriptions;
    assert_eq!(rows.len(), 1);
    rows.pop().unwrap()
}

#[tokio::test]
async fn native_retry_observations_reach_the_public_view_and_clear_after_readiness() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.native_runtime().await;
    let app = application(&fixture, &runtime);
    tokio::time::timeout(
        Duration::from_secs(10),
        fixture.post(&fixture.root, "observed first carrier failure"),
    )
    .await
    .expect("initial subscription activity committed");
    let first = tokio::time::timeout(
        Duration::from_secs(10),
        runtime.requests.lock().await.recv(),
    )
    .await
    .expect("first native attempt reached the controlled carrier")
    .unwrap();
    let push = first.payload.push_id;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    runtime
        .completions
        .send(DeliveryOutcome::NotSubmitted {
            retryable: true,
            reason: "first native refusal".into(),
        })
        .unwrap();
    let first_observed = observed_reason(&fixture, &push, "first native refusal").await;
    let first_held_at = fixture.clock.now();
    assert_eq!(first_observed.held_since, Some(first_held_at));
    assert_eq!(first_observed.scheduled_retry_at, None);
    // The public view reconciles inline, so inspect only after the carrier is
    // answered and readiness is Wakeable, never while an attempt is held open.
    let held = tokio::time::timeout(Duration::from_secs(10), view(&app, &fixture))
        .await
        .expect("public view observed the committed readiness hold");
    assert_eq!(
        held.pending_count, 1,
        "allocated Held custody vanished from pendingCount"
    );
    assert_eq!(held.held_since, Some(first_held_at));
    assert_eq!(held.next_retry_at, None);

    // An indefinite readiness hold has no scheduled retry.
    fixture.clock.advance_without_tokio_time(5);
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    // Another readiness pass can start the attempt before this test's own
    // reconciliation is queued. Receive the actual request to fix that ordering.
    let preceding_reconcile = tokio::time::timeout(
        Duration::from_secs(10),
        runtime
            .service
            .request_reconcile_receipt(fixture.reader.clone()),
    )
    .await
    .expect("preceding readiness pass queued");
    let second = tokio::time::timeout(
        Duration::from_secs(10),
        runtime.requests.lock().await.recv(),
    )
    .await
    .expect("preceding readiness pass started the second native attempt")
    .unwrap();
    assert_eq!(second.payload.push_id, push);
    // Queue the same All/Refresh pass without waiting behind the attempt whose
    // completion this test must supply. The receipt is joined after the answer.
    let second_reconcile = tokio::time::timeout(
        Duration::from_secs(10),
        runtime
            .service
            .request_reconcile_receipt(fixture.reader.clone()),
    )
    .await
    .expect("reconciliation queued behind the held native attempt");
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    runtime
        .completions
        .send(DeliveryOutcome::NotSubmitted {
            retryable: true,
            reason: "second native refusal".into(),
        })
        .unwrap();
    let second_observed = observed_reason(&fixture, &push, "second native refusal").await;
    for receipt in [preceding_reconcile, second_reconcile] {
        tokio::time::timeout(Duration::from_secs(10), receipt)
            .await
            .expect("readiness reconciliation finished after the carrier answer")
            .unwrap()
            .unwrap();
    }
    assert!(matches!(
        runtime.requests.lock().await.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    assert_eq!(
        second_observed.held_since,
        Some(first_held_at),
        "later Held overwrote first observation"
    );
    assert_eq!(second_observed.scheduled_retry_at, None);
    let delivery_snapshot = fixture
        .interactions
        .get_push_record(&push)
        .await
        .unwrap()
        .unwrap();

    fixture.clock.advance_without_tokio_time(1);
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    let final_reconcile = tokio::time::timeout(
        Duration::from_secs(10),
        runtime
            .service
            .request_reconcile_receipt(fixture.reader.clone()),
    )
    .await
    .expect("final readiness reconciliation queued");
    let third = tokio::time::timeout(
        Duration::from_secs(10),
        runtime.requests.lock().await.recv(),
    )
    .await
    .expect("final readiness pass started the third native attempt")
    .unwrap();
    assert_eq!(third.payload.push_id, push);
    runtime.await_settled().await;
    tokio::time::timeout(Duration::from_secs(10), final_reconcile)
        .await
        .expect("final readiness reconciliation finished after acceptance")
        .unwrap()
        .unwrap();
    let delivered = tokio::time::timeout(Duration::from_secs(10), view(&app, &fixture))
        .await
        .expect("public view observed the accepted native outcome");
    assert_eq!(delivered.pending_count, 0);
    assert_eq!(delivered.held_since, None);
    assert_eq!(
        delivered.next_retry_at, None,
        "next outcome retained a stale scheduled retry"
    );
    assert_eq!(
        delivered.last_outcome,
        Some(SubscriptionDeliveryOutcome::Accepted)
    );
    let terminal = fixture
        .interactions
        .get_push_record(&push)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(terminal.header_facts, delivery_snapshot.header_facts);
    assert_eq!(terminal.activity, delivery_snapshot.activity);
    assert_eq!(
        runtime
            .completed_native_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        3,
        "each readiness window completed exactly one carrier attempt"
    );
    assert!(matches!(
        runtime.requests.lock().await.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    drop(app);
    tokio::time::timeout(Duration::from_secs(10), runtime.close())
        .await
        .expect("native observation fixture shut down after every carrier answer");
}

#[tokio::test]
async fn latest_observed_outcome_wins_over_newer_allocation_and_counts_both_held_items() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.native_runtime().await;
    let app = application(&fixture, &runtime);
    fixture.post(&fixture.root, "older allocated item").await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    let older = request.payload.push_id;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    runtime
        .completions
        .send(DeliveryOutcome::NotSubmitted {
            retryable: false,
            reason: "older first refusal".into(),
        })
        .unwrap();
    observed_reason(&fixture, &older, "older first refusal").await;
    let older_observed_at = fixture
        .clock
        .now()
        .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    set_observation_time(&fixture, &older, "malformed-private-observation-time").await;
    assert!(
        app.board()
            .thread_subscriptions(ThreadSubscriptionsRequest {
                actor: fixture.reader.clone(),
            })
            .await
            .is_err(),
        "a corrupt observation was silently coerced"
    );
    fixture.clock.advance_without_tokio_time(20);
    fixture.post(&fixture.root, "newer allocated item").await;
    let newer = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let records = fixture
                .interactions
                .list_held_mail_push_records(&request.target)
                .await
                .unwrap();
            if let Some(record) = records.into_iter().find(|record| record.push_id != older) {
                break record;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("second real SDK Held item");
    assert_ne!(newer.push_id, older);
    // The real allocator has made the newer item despite the older view-only fault.
    set_observation_time(&fixture, &older, &older_observed_at).await;
    assert_eq!(view(&app, &fixture).await.pending_count, 2);

    // The older item's backoff expires while the newer item's readiness check is recent.
    fixture.clock.advance_without_tokio_time(10);
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    let driving = runtime.drive_native_due();
    let retried = tokio::time::timeout(
        Duration::from_secs(10),
        runtime.requests.lock().await.recv(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(retried.payload.push_id, older);
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    runtime
        .completions
        .send(DeliveryOutcome::NotSubmitted {
            retryable: false,
            reason: "older observed last".into(),
        })
        .unwrap();
    let observed = observed_reason(&fixture, &older, "older observed last").await;
    driving.await.unwrap().unwrap();
    assert_eq!(observed.recorded_at, Some(fixture.clock.now()));
    let latest = view(&app, &fixture).await;
    assert_eq!(latest.pending_count, 2);
    assert_eq!(
        latest.last_outcome,
        Some(SubscriptionDeliveryOutcome::NotSubmitted {
            retryable: false,
            reason: "older observed last".into(),
        })
    );
    drop(app);
    runtime.close().await;
}

#[tokio::test]
async fn failed_delivery_retries_at_its_scheduled_time_then_clears_the_public_fields() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.native_runtime().await;
    let app = application(&fixture, &runtime);
    fixture
        .post(&fixture.root, "failed delivery with backoff")
        .await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    let push = request.payload.push_id;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    runtime
        .completions
        .send(DeliveryOutcome::NotSubmitted {
            retryable: false,
            reason: "backoff refusal".into(),
        })
        .unwrap();
    let observed = observed_reason(&fixture, &push, "backoff refusal").await;
    let until =
        chrono::DateTime::from_timestamp_millis(fixture.clock.now().timestamp_millis() + 30_000)
            .unwrap();
    assert_eq!(observed.scheduled_retry_at, Some(until));
    let held = view(&app, &fixture).await;
    assert_eq!(held.held_since, Some(fixture.clock.now()));
    assert_eq!(held.next_retry_at, Some(until));
    fixture.clock.advance_without_tokio_time(25);
    runtime.drive_native_due().await.unwrap().unwrap();
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "backoff retried at t-5"
    );
    fixture.clock.advance_without_tokio_time(5);
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    let driving = runtime.drive_native_due();
    let retried = tokio::time::timeout(
        Duration::from_secs(10),
        runtime.requests.lock().await.recv(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(retried.payload.push_id, push);
    runtime.await_settled().await;
    driving.await.unwrap().unwrap();
    let delivered = view(&app, &fixture).await;
    assert_eq!(delivered.pending_count, 0);
    assert_eq!(delivered.held_since, None);
    assert_eq!(delivered.next_retry_at, None);
    assert_eq!(
        delivered.last_outcome,
        Some(SubscriptionDeliveryOutcome::Accepted)
    );
    drop(app);
    runtime.close().await;
}

#[tokio::test]
async fn a_resolved_membership_stays_visible_while_its_allocated_item_is_held() {
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.native_runtime().await;
    let app = application(&fixture, &runtime);
    let message_board::Identity::Session { session: target } = &fixture.reader else {
        panic!("session reader fixture");
    };
    let target: collaboration_protocol::SessionRef = target.clone().try_into().unwrap();
    fixture
        .post(&fixture.root, "allocated before resolution")
        .await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if fixture
                .interactions
                .held_mail_push_ids(&target)
                .await
                .unwrap()
                .len()
                == 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("actual SDK Held allocation");
    assert_eq!(view(&app, &fixture).await.pending_count, 1);
    fixture
        .resolve(
            message_board::ThreadResolveRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                reason: "resolve while the frozen item is held"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                thread: fixture.root.clone().into(),
                actor: human("board-owner"),
                acting_for: None,
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    assert_eq!(
        fixture
            .interactions
            .held_mail_push_ids(&target)
            .await
            .unwrap()
            .len(),
        1
    );
    let raw = runtime
        .service
        .request_reconcile_receipt(fixture.reader.clone())
        .await
        .await
        .unwrap()
        .unwrap();
    assert!(
        raw.records.is_empty(),
        "native custody was fed into allocation records"
    );
    assert_eq!(raw.resolved_view_candidates.len(), 1);
    let mut rows = app
        .board()
        .thread_subscriptions(ThreadSubscriptionsRequest {
            actor: fixture.reader.clone(),
        })
        .await
        .unwrap()
        .subscriptions;
    assert_eq!(
        rows.len(),
        1,
        "resolved membership with actual Held custody disappeared before view decoration"
    );
    let draining = rows.pop().unwrap();
    assert_eq!(
        draining.state,
        collaboration_protocol::ThreadSubscriptionState::Draining
    );
    assert_eq!(draining.pending_count, 1);
    assert!(draining.held_since.is_some());
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.clock.advance_without_tokio_time(30);
    let request = tokio::time::timeout(
        Duration::from_secs(10),
        runtime.requests.lock().await.recv(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(request.target, target);
    runtime.await_settled().await;
    let settled = app
        .board()
        .thread_subscriptions(ThreadSubscriptionsRequest {
            actor: fixture.reader.clone(),
        })
        .await
        .unwrap();
    assert!(
        settled.subscriptions.is_empty(),
        "resolved candidate survived terminal settlement"
    );
    drop(app);
    runtime.close().await;
}
