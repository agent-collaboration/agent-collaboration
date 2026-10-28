//! Real waiter disconnect, physical owner reopen and SDK interruption preserve per-item custody.
use super::native_follow_mail_proofs::{follow_application, frozen_follow};
use super::*;
use crate::subscription_delivery::subscription_service::PollHandoverStage;
use crate::{SubscriptionWaitFilter, SubscriptionWaitResult};
use fleet_sync_protocol::RecipientOutcome;
use message_board::*;

async fn held_item(
    fixture: &OwnerFixture,
    runtime: &OwnerRuntime,
    publisher: &crate::NotificationPublisher,
) -> fleet_sync_protocol::ItemId {
    runtime.owner_barrier(&fixture.reader).await;
    tokio::time::timeout(Duration::from_secs(6), async {
        while frozen_follow(fixture).await.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    publisher.wait_project_idle_for_test(&fixture.project).await;
    frozen_follow(fixture).await.first().unwrap().id()
}

#[tokio::test]
async fn a_disconnected_poll_waiter_keeps_the_item_for_the_next_wait_without_unknown() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture
        .policy(&fixture.root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
        .await;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    fixture
        .post(&fixture.root, "disconnect before the actual handover")
        .await;
    let item = held_item(&fixture, &runtime, &publisher).await;
    let push: collaboration_protocol::PushId = item.as_uuid().to_string().try_into().unwrap();
    let witness = runtime
        .service
        .pause_poll_handover_for_test(push.clone(), PollHandoverStage::BeforeSend);
    let service = runtime.service.clone();
    let reader = fixture.reader.clone();
    let waiting = tokio::spawn(async move {
        service
            .wait(reader, SubscriptionWaitFilter::All, 60, usize::MAX)
            .await
    });
    tokio::time::timeout(Duration::from_secs(6), witness.entered)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture
            .interactions
            .get_push_record(&push)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Attempted
    );
    waiting.abort();
    assert!(waiting.await.unwrap_err().is_cancelled());
    witness.resume.send(()).unwrap();
    let held = fixture
        .await_push_state(&push, PushDeliveryState::Held)
        .await;
    assert!(matches!(
        held.last_outcome.unwrap().outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: true,
            ..
        }
    ));
    assert!(
        fixture
            .interactions
            .pending_poll_item(item)
            .await
            .unwrap()
            .is_some()
    );
    assert!(matches!(
        native.runtime().local_outcome_for_test(item).await.unwrap(),
        Some(RecipientOutcome::Held)
    ));
    let next = tokio::time::timeout(
        Duration::from_secs(6),
        runtime.service.wait(
            fixture.reader.clone(),
            SubscriptionWaitFilter::All,
            60,
            usize::MAX,
        ),
    )
    .await;
    if matches!(&next, Ok(Ok(Some(_)))) {
        fixture
            .await_push_state(&push, PushDeliveryState::Delivered)
            .await;
    }
    let pending = fixture.interactions.pending_poll_item(item).await.unwrap();
    let invoked = runtime.requests.lock().await.try_recv().is_ok();
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    let SubscriptionWaitResult::Notice { push_id, batch, .. } = next
        .expect("next wait did not recover the disconnected item")
        .unwrap()
        .unwrap()
    else {
        panic!("session notice");
    };
    assert_eq!(push_id, push);
    assert_eq!(batch.roots.len(), 1);
    assert!(pending.is_none());
    assert!(!invoked);
}

#[tokio::test]
async fn held_poll_policy_and_original_identity_survive_physical_owner_reopen() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture
        .policy(&fixture.root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
        .await;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    fixture
        .post(&fixture.root, "persist while no waiter is registered")
        .await;
    let item = held_item(&fixture, &runtime, &publisher).await;
    let push: collaboration_protocol::PushId = item.as_uuid().to_string().try_into().unwrap();
    let original = fixture
        .interactions
        .pending_poll_item(item)
        .await
        .unwrap()
        .unwrap();
    let frozen = serde_json::to_value(frozen_follow(&fixture).await.first().unwrap()).unwrap();
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    let fixture = fixture.reopen_native(native).await;
    fixture.clock.advance_without_tokio_time(60);
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    assert_eq!(
        fixture.interactions.pending_poll_item(item).await.unwrap(),
        Some(original.clone())
    );
    assert!(
        fixture
            .interactions
            .get_push_record(&push)
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        native.runtime().local_outcome_for_test(item).await.unwrap(),
        Some(RecipientOutcome::Held)
    ));
    let result = tokio::time::timeout(
        Duration::from_secs(6),
        runtime.service.wait(
            fixture.reader.clone(),
            SubscriptionWaitFilter::All,
            60,
            usize::MAX,
        ),
    )
    .await;
    let record = if matches!(&result, Ok(Ok(Some(_)))) {
        Some(
            fixture
                .await_push_state(&push, PushDeliveryState::Delivered)
                .await,
        )
    } else {
        None
    };
    let remaining = fixture.interactions.pending_poll_item(item).await.unwrap();
    let frozen_after = frozen_follow(&fixture).await;
    let invoked = runtime.requests.lock().await.try_recv().is_ok();
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    let SubscriptionWaitResult::Notice { push_id, batch, .. } = result
        .expect("cold wait lost persisted Poll custody")
        .unwrap()
        .unwrap()
    else {
        panic!("session notice");
    };
    assert_eq!(push_id, push);
    assert_eq!(batch.roots.len(), 1);
    assert_eq!(record.unwrap().created_at, original.received_at);
    assert_eq!(frozen_after.len(), 1);
    assert_eq!(
        serde_json::to_value(frozen_after.first().unwrap()).unwrap(),
        frozen
    );
    assert!(remaining.is_none());
    assert!(!invoked);
}

fn includes_root(
    item: &fleet_sync_protocol::DeliveryItem<board_sync_protocol::BoardDeliveryTypes>,
    root: &ThreadId,
) -> bool {
    matches!(item.body(),board_sync_protocol::BoardDeliveryBody::Notification(body)
        if matches!(&body.source,collaboration_mail_protocol::NotificationSource::FollowBatch{roots,..} if roots.iter().any(|range|range.root()==root)))
}

#[tokio::test]
async fn interruption_after_b_handover_leaves_b_uncertain_and_c_deliverable_after_reopen() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let roots = super::follow_admission_proofs::project_poll_roots(&fixture).await;
    fixture
        .subscribe(
            ThreadSubscriptionSubscribeRequest {
                reader: fixture.reader.clone(),
                scope: SubscriptionScope::project(fixture.project.clone()),
                policy: SubscriptionPolicyPatch {
                    mode: None,
                    when_idle: None,
                    timing: SubscriptionTimingPatch {
                        quiet_seconds: Some(1),
                        cap_seconds: Some(1),
                    },
                    lifetime_seconds: None,
                },
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    let first = roots.first().unwrap();
    let second = roots.get(1).unwrap();
    let third = roots.get(2).unwrap();
    fixture
        .post(first, "A is independently handed over first")
        .await;
    fixture.clock.advance_without_tokio_time(1);
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    held_item(&fixture, &runtime, &publisher).await;
    let a = tokio::time::timeout(
        Duration::from_secs(6),
        runtime.service.wait(
            fixture.reader.clone(),
            SubscriptionWaitFilter::All,
            60,
            usize::MAX,
        ),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    let SubscriptionWaitResult::Notice { push_id: a_id, .. } = a else {
        panic!("A notice");
    };
    fixture
        .await_push_state(&a_id, PushDeliveryState::Delivered)
        .await;
    // Both later roots become due together, so a wrong multi-root Poll allocator
    // really puts B/C into one item and reaches the interruption checkpoint.
    fixture
        .post(second, "B is interrupted after external handover")
        .await;
    fixture
        .post(third, "C must remain independently deliverable")
        .await;
    fixture.clock.advance_without_tokio_time(1);
    runtime
        .service
        .reconcile_reader(fixture.reader.clone())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let items = frozen_follow(&fixture).await;
            if items.iter().any(|item| includes_root(item, third)) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    publisher.wait_project_idle_for_test(&fixture.project).await;
    let frozen = frozen_follow(&fixture).await;
    let b = frozen
        .iter()
        .find(|item| includes_root(item, second))
        .unwrap()
        .id();
    let c = frozen
        .iter()
        .find(|item| includes_root(item, third))
        .unwrap()
        .id();
    let b_id: collaboration_protocol::PushId = b.as_uuid().to_string().try_into().unwrap();
    let witness = runtime
        .service
        .pause_poll_handover_for_test(b_id.clone(), PollHandoverStage::AfterSend);
    let service = runtime.service.clone();
    let reader = fixture.reader.clone();
    let filter = SubscriptionWaitFilter::Roots(vec![second.clone().into()]);
    let b_wait = tokio::spawn(async move { service.wait(reader, filter, 60, usize::MAX).await });
    tokio::time::timeout(Duration::from_secs(6), witness.entered)
        .await
        .unwrap()
        .unwrap();
    let handed_b = tokio::time::timeout(Duration::from_secs(6), b_wait)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture
            .interactions
            .get_push_record(&b_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Attempted
    );
    let abandoned = native.abort_for_crash().await.unwrap();
    drop(abandoned);
    witness.resume.send(()).unwrap();
    runtime.owner_barrier(&fixture.reader).await;
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    // The SDK's established drive lease is 30 seconds; expire that lease while
    // the 24-hour membership stays valid. This is driver abort, not SIGKILL.
    fixture.clock.advance_without_tokio_time(60);
    let fixture = fixture.reopen_messages().await;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    let b_record = fixture
        .await_push_state(&b_id, PushDeliveryState::OutcomeUnknown)
        .await;
    let b_outcome = native.runtime().local_outcome_for_test(b).await.unwrap();
    let result_c = tokio::time::timeout(
        Duration::from_secs(6),
        runtime.service.wait(
            fixture.reader.clone(),
            SubscriptionWaitFilter::Roots(vec![third.clone().into()]),
            60,
            usize::MAX,
        ),
    )
    .await;
    let delivered_c = match &result_c {
        Ok(Ok(Some(SubscriptionWaitResult::Notice { push_id, .. }))) => Some(
            fixture
                .await_push_state(push_id, PushDeliveryState::Delivered)
                .await,
        ),
        _ => None,
    };
    let invoked = runtime.requests.lock().await.try_recv().is_ok();
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    let SubscriptionWaitResult::Notice {
        push_id: c_id,
        batch: c_batch,
        ..
    } = result_c
        .expect("interrupting B stranded independent C custody")
        .unwrap()
        .unwrap()
    else {
        panic!("C notice");
    };
    assert!(delivered_c.is_some());
    assert_eq!(c_id.as_str(), c.as_uuid().to_string());
    assert_eq!(c_batch.roots.len(), 1);
    assert_eq!(&c_batch.roots.first().unwrap().thread_id, third);
    assert!(matches!(b_outcome, Some(RecipientOutcome::Uncertain)));
    assert_eq!(
        b_record.last_outcome.unwrap().outcome,
        DeliveryOutcome::Unknown
    );
    let SubscriptionWaitResult::Notice {
        push_id: observed_b,
        batch: b_batch,
        ..
    } = handed_b
    else {
        panic!("B notice");
    };
    assert_eq!(observed_b, b_id);
    assert_eq!(b_batch.roots.len(), 1);
    assert!(!invoked);
}
