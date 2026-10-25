use super::owner_fixture::*;
use crate::service_identity::subscription_delivery::subscription_service::OwnerObservation;
use crate::{LoadPolicy, TargetPresence};
use message_board::{EndReason, SubscriptionMode, SubscriptionScope, WhenIdle};

#[tokio::test]
async fn hold_keeps_each_arrival_in_a_frozen_item_and_delivers_it_once_on_running() {
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.native_runtime().await;
    let mut positions = Vec::new();
    for (index, text) in ["First held", "Second held", "Third held"]
        .into_iter()
        .enumerate()
    {
        positions.push(fixture.post(&fixture.root, text).await);
        let held = fixture.held_native_pushes(index + 1).await;
        let newest = held.last().unwrap();
        runtime.observe(|event| matches!(event,
            OwnerObservation::NativeHeldChecked { item, running: false } if item == &newest.push_id
        )).await;
    }
    runtime.owner_barrier(&fixture.reader).await;
    let held = fixture.held_native_pushes(3).await;
    assert_eq!(
        fixture
            .public_subscription_view(&runtime)
            .await
            .pending_count,
        3
    );
    assert!(runtime.requests.lock().await.try_recv().is_err());
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.clock.advance_without_tokio_time(29);
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    fixture.clock.advance_without_tokio_time(1);
    let mut received = std::collections::BTreeSet::new();
    let mut covered = Vec::new();
    for _ in 0..3 {
        let request = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            runtime.requests.lock().await.recv().await.unwrap()
        })
        .await
        .unwrap();
        assert!(
            received.insert(request.payload.push_id.as_str().to_owned()),
            "one frozen item was delivered twice"
        );
        assert_eq!(request.payload.load_policy, LoadPolicy::LoadedOnly);
        assert!(request.payload.line.as_str().contains("held since"));
        let record = fixture
            .interactions
            .get_push_record(&request.payload.push_id)
            .await
            .unwrap()
            .unwrap();
        let activity = record.activity.unwrap();
        assert!(activity.held);
        assert_eq!(activity.ranges.len(), 1);
        assert_eq!(activity.ranges[0].from, activity.ranges[0].through);
        covered.push(activity.ranges[0].through);
        runtime
            .completions
            .send(collaboration_protocol::DeliveryOutcome::Started)
            .unwrap();
    }
    for record in &held {
        assert!(received.contains(record.push_id.as_str()));
        fixture
            .await_push_state(
                &record.push_id,
                collaboration_protocol::PushDeliveryState::Delivered,
            )
            .await;
    }
    assert_eq!(covered.len(), positions.len());
    for position in positions {
        assert_eq!(
            covered.iter().filter(|actual| **actual == position).count(),
            1
        );
    }
    assert!(runtime.requests.lock().await.try_recv().is_err());
    assert_eq!(
        runtime
            .completed_native_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        3
    );
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn wake_uses_may_load_only_for_a_wakeable_target() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Deliver,
            WhenIdle::Wake,
            0,
            0,
        )
        .await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "Wake me").await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    assert_eq!(request.payload.load_policy, LoadPolicy::MayLoad);
    runtime.await_settled().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.post(&fixture.root, "Running").await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    assert_eq!(request.payload.load_policy, LoadPolicy::LoadedOnly);
    runtime.await_settled().await;
    runtime.close().await;
}

#[tokio::test]
async fn unreachable_wake_is_held_with_visible_reason() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Deliver,
            WhenIdle::Wake,
            0,
            0,
        )
        .await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Unreachable {
        reason: "closed terminal".to_owned(),
    };
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "Pending").await;
    let held = fixture.held_native_pushes(1).await.pop().unwrap();
    let view = fixture.public_subscription_view(&runtime).await;
    let Some(collaboration_protocol::DeliveryReceipt {
        outcome:
            collaboration_protocol::DeliveryOutcome::NotSubmitted {
                retryable: true,
                reason,
            },
        ..
    }) = held.last_outcome
    else {
        panic!("unreachable follow must retain its known-unsent receipt");
    };
    assert_eq!(reason, "target is not running; follow batch held");
    assert_eq!(
        view.last_outcome,
        Some(message_board::SubscriptionDeliveryOutcome::NotSubmitted {
            retryable: true,
            reason
        })
    );
    assert_eq!(view.pending_count, 1);
    assert!(view.held_since.is_some());
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn drop_then_push_leaves_both_messages_unread() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Deliver,
            WhenIdle::Drop,
            0,
            0,
        )
        .await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "Skipped").await;
    runtime
        .wait_for_activity(&fixture.reader, |event| {
            matches!(event, OwnerObservation::DueRoots(1))
        })
        .await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.post(&fixture.root, "Pushed").await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    let record = fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        record.header_facts,
        collaboration_protocol::PushHeaderFacts::SubscriptionActivity {
            message_count: 1,
            ..
        }
    ));
    runtime.await_settled().await;
    assert_eq!(fixture.unread_count().await, 2);
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn expiry_without_activity_sends_one_stored_renew_command() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.native_runtime().await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture.clock.advance(24 * 60 * 60).await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    assert!(
        request
            .payload
            .line
            .as_str()
            .starts_with("🧵 Router: subscription expired")
    );
    assert!(!request.payload.line.as_str().contains("subscribe"));
    let record = fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.kind,
        collaboration_protocol::PushKind::SubscriptionExpiry
    );
    assert!(
        record
            .body
            .unwrap()
            .contains("thread subscribe --thread-id")
    );
    runtime
        .completions
        .send(collaboration_protocol::DeliveryOutcome::Started)
        .unwrap();
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
    assert_eq!(record.state().end_reason(), Some(EndReason::Expired));
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
async fn unknown_outcome_advances_delivered_and_preserves_evidence() {
    let fixture = OwnerFixture::new().await;
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "Uncertain").await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    runtime
        .completions
        .send(collaboration_protocol::DeliveryOutcome::Unknown)
        .unwrap();
    fixture
        .await_push_state(
            &request.payload.push_id,
            collaboration_protocol::PushDeliveryState::OutcomeUnknown,
        )
        .await;
    runtime.owner_barrier(&fixture.reader).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    let stored = fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.delivery_state,
        collaboration_protocol::PushDeliveryState::OutcomeUnknown
    );
    let record = fixture.public_subscription_view(&runtime).await;
    assert!(matches!(
        record.last_outcome,
        Some(message_board::SubscriptionDeliveryOutcome::Unknown { .. })
    ));
    assert_eq!(fixture.unread_count().await, 1);
    runtime.close().await;
}
use super::super::SubscriptionClock;
