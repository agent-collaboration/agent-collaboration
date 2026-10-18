use super::owner_fixture::*;
use crate::service_identity::subscription_delivery::subscription_service::OwnerObservation;
use crate::service_identity::subscription_delivery::{
    SubscriptionClock, SubscriptionWaitFilter, SubscriptionWaitResult,
};
use message_board::{EndReason, SubscriptionMode, SubscriptionScope, WhenIdle};
use sqlx::Connection;

#[tokio::test(start_paused = true)]
async fn resumed_reader_batches_held_roots_with_staggered_presence_checks() {
    let fixture = OwnerFixture::new().await;
    let second_root = fixture.add_root().await;
    let poll_root = fixture.add_root().await;
    fixture
        .policy(&poll_root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
        .await;
    *fixture.presence.0.lock().unwrap() = crate::TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    fixture.post(&fixture.root, "First held root").await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::Held))
        .await;
    runtime.owner_barrier(&fixture.reader).await;

    fixture.clock.advance(1).await;
    fixture.post(&second_root, "Second held root").await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::Held))
        .await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture
        .post(&poll_root, "Poll activity stays pending")
        .await;

    // Only the first root's presence interval has elapsed when the reader resumes.
    *fixture.presence.0.lock().unwrap() = crate::TargetPresence::Running;
    fixture.clock.advance(29).await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    let record = fixture
        .push_store
        .lock()
        .await
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    let activity = record.activity.unwrap();
    assert!(activity.held);
    assert_eq!(activity.ranges.len(), 2);
    for root in [&fixture.root, &second_root] {
        assert_eq!(
            activity
                .ranges
                .iter()
                .filter(|range| &range.thread_id == root)
                .count(),
            1
        );
    }
    assert!(request.payload.line.as_str().contains(" · 2 threads · "));
    runtime.await_settled().await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    let poll = fixture
        .store
        .lock()
        .await
        .get_thread_subscription_record(
            &fixture.reader,
            &SubscriptionScope::thread(poll_root),
            fixture.clock.now(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(poll.roots()[0].pending_count(), 1);
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn mode_change_during_flight_settles_original_push_and_polls_released_backlog() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.runtime().await;
    let original_post = fixture
        .post(&fixture.root, "Original in-flight activity")
        .await;
    let original_request = runtime.requests.lock().await.recv().await.unwrap();
    let original_record = fixture
        .push_store
        .lock()
        .await
        .get_push_record(&original_request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    let scope = SubscriptionScope::thread(fixture.root.clone());
    let original_generation = fixture
        .store
        .lock()
        .await
        .get_thread_subscription_record(&fixture.reader, &scope, fixture.clock.now())
        .await
        .unwrap()
        .unwrap()
        .generation();
    fixture
        .policy(&fixture.root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
        .await;
    let later_post = fixture.post(&fixture.root, "Later poll activity").await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.await_settled().await;
    runtime.owner_barrier(&fixture.reader).await;

    let settled_record = fixture
        .push_store
        .lock()
        .await
        .get_push_record(&original_request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(settled_record.push_id, original_record.push_id);
    assert_eq!(settled_record.activity, original_record.activity);
    assert_eq!(
        settled_record.delivery_state,
        collaboration_protocol::PushDeliveryState::Delivered
    );
    assert_eq!(
        settled_record.last_outcome.unwrap().outcome,
        collaboration_protocol::DeliveryOutcome::Started
    );
    let updated_subscription = fixture
        .store
        .lock()
        .await
        .get_thread_subscription_record(&fixture.reader, &scope, fixture.clock.now())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_subscription.policy().mode(), SubscriptionMode::Poll);
    assert_ne!(updated_subscription.generation(), original_generation);
    assert_eq!(updated_subscription.last_outcome(), None);
    assert_eq!(updated_subscription.roots()[0].pending_count(), 2);
    assert!(runtime.requests.lock().await.try_recv().is_err());

    let result = runtime
        .service
        .wait(
            fixture.reader.clone(),
            SubscriptionWaitFilter::Roots(vec![(fixture.root.clone()).into()]),
            60,
            usize::MAX,
        )
        .await
        .unwrap()
        .unwrap();
    let SubscriptionWaitResult::Notice { push_id, batch, .. } = result else {
        panic!("session poll must return a stored notice");
    };
    assert_ne!(push_id, original_request.payload.push_id);
    assert_eq!(batch.roots.len(), 1);
    assert_eq!(batch.roots[0].post_count, 2);
    assert_eq!(batch.roots[0].from, original_post);
    assert_eq!(batch.roots[0].through, later_post);
    assert!(
        batch.roots[0].through.epoch == original_post.epoch
            && batch.roots[0].through.position > original_post.position
    );
    runtime
        .observe(|event| matches!(event, OwnerObservation::Settled))
        .await;
    runtime.owner_barrier(&fixture.reader).await;
    let final_subscription = fixture
        .store
        .lock()
        .await
        .get_thread_subscription_record(&fixture.reader, &scope, fixture.clock.now())
        .await
        .unwrap()
        .unwrap();
    assert!(final_subscription.roots().is_empty());
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn one_owner_serializes_push_and_wait_and_does_not_hold_the_board_lock() {
    let fixture = OwnerFixture::new().await;
    let poll_root = fixture.add_root().await;
    fixture
        .policy(&poll_root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
        .await;
    let runtime = fixture.runtime().await;
    fixture.post(&fixture.root, "Push").await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    fixture.post(&poll_root, "Poll").await; // This real write proves the route await released the board lock.
    let service = runtime.service.clone();
    let reader = fixture.reader.clone();
    let mut wait = tokio::spawn(async move {
        service
            .wait(
                reader,
                SubscriptionWaitFilter::Roots(vec![(poll_root).into()]),
                60,
                usize::MAX,
            )
            .await
            .unwrap()
    });
    assert!(!wait.is_finished());
    runtime.await_settled().await;
    let notice = (&mut wait).await.unwrap().unwrap();
    let SubscriptionWaitResult::Notice { push_id, batch, .. } = notice else {
        panic!("session waits have stored notices");
    };
    assert_ne!(push_id, request.payload.push_id);
    assert_eq!(batch.roots.len(), 1);
    runtime
        .observe(|event| matches!(event, OwnerObservation::Settled))
        .await;
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn twenty_root_limit_drains_remaining_due_roots_immediately() {
    let fixture = OwnerFixture::new().await;
    let mut roots = vec![fixture.root.clone()];
    for _ in 0..20 {
        roots.push(fixture.add_root().await);
    }
    for root in &roots {
        fixture.post(root, "Pending").await;
    }
    let runtime = fixture.runtime().await;
    let first = runtime.requests.lock().await.recv().await.unwrap();
    let first_record = fixture
        .push_store
        .lock()
        .await
        .get_push_record(&first.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    let first_ranges = first_record.activity.unwrap().ranges;
    assert_eq!(first_ranges.len(), 20);
    runtime.await_settled().await;
    let second = runtime.requests.lock().await.recv().await.unwrap();
    let second_record = fixture
        .push_store
        .lock()
        .await
        .get_push_record(&second.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    let second_ranges = second_record.activity.unwrap().ranges;
    assert_eq!(second_ranges.len(), 1);
    assert!(
        !first_ranges
            .iter()
            .any(|range| range.thread_id == second_ranges[0].thread_id)
    );
    runtime.await_settled().await;
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn arrival_during_flight_gets_its_own_quiet_window_and_elapsed_cap() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Deliver,
            WhenIdle::Hold,
            120,
            600,
        )
        .await;
    let runtime = fixture.runtime().await;
    fixture.post(&fixture.root, "Original").await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture.clock.advance(120).await;
    let first = runtime.requests.lock().await.recv().await.unwrap();
    fixture.post(&fixture.root, "First residual").await;
    for _ in 0..6 {
        fixture.clock.advance(90).await;
        fixture.post(&fixture.root, "Residual extends quiet").await;
    }
    fixture.clock.advance(60).await; // Cap from the first residual has elapsed while the first push was in flight.
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.await_settled().await;
    let second = runtime.requests.lock().await.recv().await.unwrap();
    assert_ne!(first.payload.push_id, second.payload.push_id);
    assert!(second.payload.line.as_str().contains("7 messages"));
    runtime.await_settled().await;
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn cancel_during_flight_settles_the_passed_range_and_prevents_later_pushes() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.runtime().await;
    fixture.post(&fixture.root, "In flight").await;
    runtime.requests.lock().await.recv().await.unwrap();
    fixture
        .unsubscribe(
            message_board::ThreadSubscriptionUnsubscribeRequest {
                reader: fixture.reader.clone(),
                scope: SubscriptionScope::thread(fixture.root.clone()),
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    runtime.await_settled().await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::Stopped))
        .await;
    fixture.post(&fixture.root, "After cancel").await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    assert_eq!(fixture.unread_count().await, 2);
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn board_settlement_failure_retries_without_redelivery() {
    let fixture = OwnerFixture::new().await;
    let mut observer = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(fixture.directory.path().join("automation.sqlite")),
    )
    .await
    .unwrap();
    sqlx::query("CREATE TRIGGER fail_owner_settle BEFORE UPDATE OF payload ON subscription_delivery_progress WHEN json_extract(OLD.payload, '$.roots[0].window.selected') IS NOT NULL AND json_extract(NEW.payload, '$.roots[0].window') IS NULL BEGIN SELECT RAISE(FAIL, 'injected settlement failure'); END").execute(&mut observer).await.unwrap();
    let runtime = fixture.runtime().await;
    fixture
        .post(&fixture.root, "Accepted before recording")
        .await;
    runtime.requests.lock().await.recv().await.unwrap();
    runtime
        .completions
        .send(collaboration_protocol::DeliveryOutcome::Started)
        .unwrap();
    runtime
        .observe(|event| matches!(event, OwnerObservation::SettlementRetry))
        .await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    sqlx::query("DROP TRIGGER fail_owner_settle")
        .execute(&mut observer)
        .await
        .unwrap();
    fixture.clock.advance(1).await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::Settled))
        .await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    observer.close().await.unwrap();
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn resolved_held_subscription_drains_then_ends_resolved() {
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = crate::TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    fixture.post(&fixture.root, "Held final activity").await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::Held))
        .await;
    fixture
        .resolve(
            message_board::ThreadResolveRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                observed_epoch: None,
                resubmits: None,
                reason: message_board::ThreadResolutionReason::try_from(
                    "delivery proof resolved".to_owned(),
                )
                .unwrap(),
                thread: (fixture.root.clone()).into(),
                actor: human("board-owner"),
                acting_for: None,
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    runtime.owner_barrier(&fixture.reader).await;
    *fixture.presence.0.lock().unwrap() = crate::TargetPresence::Running;
    fixture.clock.advance(30).await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    assert!(request.payload.line.as_str().contains("thread resolved"));
    runtime.await_settled().await;
    runtime
        .observe(|event| matches!(event, OwnerObservation::Stopped))
        .await;
    let record = fixture
        .store
        .lock()
        .await
        .get_thread_subscription_record(
            &fixture.reader,
            &SubscriptionScope::thread(fixture.root.clone()),
            fixture.clock.now(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.state().end_reason(), Some(EndReason::Resolved));
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn restart_mid_quiet_preserves_the_original_arrival_deadline() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Deliver,
            WhenIdle::Hold,
            120,
            600,
        )
        .await;
    let runtime = fixture.runtime().await;
    fixture.post(&fixture.root, "Before restart").await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture.clock.advance(60).await;
    runtime.owner_barrier(&fixture.reader).await;
    runtime.close().await;
    reopen_board(&fixture).await;
    let runtime = fixture.runtime().await;
    fixture.clock.advance(59).await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    fixture.clock.advance(1).await;
    runtime.requests.lock().await.recv().await.unwrap();
    runtime.await_settled().await;
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn restart_after_cap_delivers_immediately_with_downtime_arrivals_included() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Deliver,
            WhenIdle::Hold,
            120,
            600,
        )
        .await;
    let runtime = fixture.runtime().await;
    fixture.post(&fixture.root, "Before downtime").await;
    runtime.owner_barrier(&fixture.reader).await;
    runtime.close().await;
    for _ in 0..7 {
        fixture.clock.advance(90).await;
        fixture.post(&fixture.root, "During downtime").await;
    }
    reopen_board(&fixture).await;
    let runtime = fixture.runtime().await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    assert!(request.payload.line.as_str().contains("8 messages"));
    runtime.await_settled().await;
    runtime.close().await;
}

async fn reopen_board(fixture: &OwnerFixture) {
    let reopened = automation_storage::AutomationStore::open(
        &fixture.directory.path().join("automation.sqlite"),
    )
    .await
    .unwrap();
    let previous = std::mem::replace(&mut *fixture.push_store.lock().await, reopened);
    previous.close().await.unwrap();
    let registry = std::sync::Arc::new(crate::ProjectStoreRegistry::new(fixture.directory.path()));
    *fixture.store.lock().await =
        crate::SubscriptionWindowStore::new(registry, fixture.push_store.clone());
}
