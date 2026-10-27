use super::owner_fixture::*;
use crate::service_identity::subscription_delivery::subscription_service::OwnerObservation;
use crate::service_identity::subscription_delivery::{
    SubscriptionClock, SubscriptionWaitFilter, SubscriptionWaitResult,
};
use message_board::{SubscriptionMode, SubscriptionScope, WhenIdle};
use std::sync::Arc;
use std::time::Duration;

// Decision 771 replaces the cross-membership packet count with
// local_mail_proofs::native_follow_grouping_proofs::resumed_reader_keeps_separate_membership_items_with_staggered_presence_checks.

#[tokio::test]
async fn mode_change_during_flight_settles_original_push_and_polls_released_backlog() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let fixture = OwnerFixture::new().await;
        let runtime = fixture.native_runtime().await;
        let original_post = fixture
            .post(&fixture.root, "Original in-flight activity")
            .await;
        let original_request = runtime.requests.lock().await.recv().await.unwrap();
        let original_record = fixture
            .interactions
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
            .interactions
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
        let held_poll = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let rows = fixture
                    .interactions
                    .pending_poll_mail(
                        &super::super::subscription_push::target_session(&fixture.reader).unwrap(),
                    )
                    .await
                    .unwrap();
                if let Some(first) = rows.first()
                    && runtime.local_mail_outcome(first.item).await.is_some()
                {
                    break rows;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("new-generation Poll item reaches its first actual SDK outcome");
        let frozen =
            super::local_mail_proofs::native_follow_mail_proofs::frozen_follow(&fixture).await;
        for item in frozen
            .iter()
            .filter(|item| held_poll.iter().any(|pending| pending.item == item.id()))
        {
            let board_sync_protocol::BoardDeliveryBody::Notification(body) = item.body() else {
                continue;
            };
            if let collaboration_mail_protocol::NotificationSource::FollowBatch { roots, .. } =
                &body.source
            {
                assert!(
                    !roots.iter().any(|root| root.root() == &fixture.root
                        && root.from().position <= original_post.position
                        && root.through().position >= original_post.position),
                    "delivered original range was allocated again: {}",
                    serde_json::to_string(item).unwrap()
                );
            }
        }
        assert_eq!(held_poll.len(), 1);
        let poll_item = frozen
            .iter()
            .find(|item| item.id() == held_poll[0].item)
            .unwrap();
        let board_sync_protocol::BoardDeliveryBody::Notification(body) = poll_item.body() else {
            panic!("frozen follow notification");
        };
        let collaboration_mail_protocol::NotificationSource::FollowBatch { roots, policy, .. } =
            &body.source
        else {
            panic!("frozen follow batch");
        };
        assert_eq!(policy.mode(), SubscriptionMode::Poll);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].root(), &fixture.root);
        assert_eq!(roots[0].from(), later_post);
        assert_eq!(roots[0].through(), later_post);
        assert_eq!(roots[0].post_count(), 1);
        assert_eq!(
            runtime.local_mail_outcome(poll_item.id()).await,
            Some(fleet_sync_protocol::RecipientOutcome::Held)
        );
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
        assert_eq!(batch.roots[0].post_count, 1);
        assert_eq!(batch.roots[0].from, later_post);
        assert_eq!(batch.roots[0].through, later_post);
        assert!(
            batch.roots[0].through.epoch == original_post.epoch
                && batch.roots[0].through.position > original_post.position
        );
        assert_eq!(
            runtime.await_mail_outcome(&push_id).await,
            fleet_sync_protocol::RecipientOutcome::Delivered {
                acceptance: collaboration_mail_protocol::CarrierAcceptance::Started,
            }
        );
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
    })
    .await
    .expect("bounded native flight and Poll handover journey");
}

#[tokio::test]
async fn one_owner_serializes_push_and_wait_and_does_not_hold_the_board_lock() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let fixture = OwnerFixture::new().await;
        let poll_root = fixture.add_root().await;
        fixture
            .policy(&poll_root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
            .await;
        let runtime = fixture.native_runtime().await;
        let push_position = fixture.post(&fixture.root, "Push").await;
        let request = runtime.requests.lock().await.recv().await.unwrap();
        let poll_position = fixture.post(&poll_root, "Poll").await; // This real write proves the route await released the board lock.
        let service = runtime.service.clone();
        let reader = fixture.reader.clone();
        let wait_root = poll_root.clone();
        let mut wait = tokio::spawn(async move {
            service
                .wait(
                    reader,
                    SubscriptionWaitFilter::Roots(vec![wait_root.into()]),
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
        assert_eq!(batch.roots[0].project_id, fixture.project);
        assert_eq!(batch.roots[0].thread_id, poll_root);
        assert_eq!(batch.roots[0].from, poll_position);
        assert_eq!(batch.roots[0].through, poll_position);
        assert_eq!(batch.roots[0].post_count, 1);
        let original = fixture
            .interactions
            .get_push_record(&request.payload.push_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            original.activity.unwrap().ranges,
            vec![collaboration_protocol::PushActivityRange {
                project_id: fixture.project.clone(),
                thread_id: fixture.root.clone(),
                from: push_position,
                through: push_position,
            }]
        );
        assert!(runtime.requests.lock().await.try_recv().is_err());
        assert_eq!(
            runtime.await_mail_outcome(&push_id).await,
            fleet_sync_protocol::RecipientOutcome::Delivered {
                acceptance: collaboration_mail_protocol::CarrierAcceptance::Started,
            }
        );
        runtime.close().await;
    })
    .await
    .expect("bounded native flight and Poll handover journey");
}

// Decision 771 keeps the 20-root limit within a single membership in
// local_mail_proofs::native_follow_grouping_proofs::one_membership_drains_twenty_roots_then_the_remaining_root_without_lost_ranges.

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
    let runtime = fixture.native_runtime().await;
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
    let runtime = fixture.native_runtime().await;
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

// Decision 936 maps the former post-delivery window-settlement fault to
// native_follow_outcome_fault_proofs::a_follow_outcome_write_fault_recovers_without_a_second_native_call.
// Since 747, the frozen allocation owns coverage before native delivery.

// Decisions 468 and 823 preserve the already frozen held item's header, and
// prove its resolution drain through the native owner in
// local_mail_proofs::native_follow_cancel_proofs::a_resolved_membership_keeps_its_held_item_and_drains_the_unallocated_range.

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
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "Before restart").await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture.clock.advance(60).await;
    runtime.owner_barrier(&fixture.reader).await;
    runtime.close().await;
    let fixture = reopen_board(fixture).await;
    let runtime = fixture.native_runtime().await;
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
    let runtime = fixture.native_runtime().await;
    fixture.post(&fixture.root, "Before downtime").await;
    runtime.owner_barrier(&fixture.reader).await;
    runtime.close().await;
    for _ in 0..7 {
        fixture.clock.advance(90).await;
        fixture.post(&fixture.root, "During downtime").await;
    }
    let fixture = reopen_board(fixture).await;
    let runtime = fixture.native_runtime().await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    assert!(request.payload.line.as_str().contains("8 messages"));
    runtime.await_settled().await;
    runtime.close().await;
}

async fn reopen_board(fixture: OwnerFixture) -> OwnerFixture {
    // The delivery runtime is stopped. Close its producer's native owner too,
    // before a different registry performs the real cold file inspection.
    fixture
        .registry
        .database(&fixture.project)
        .await
        .unwrap()
        .close()
        .await;
    let mut fixture = fixture.reopen_messages().await;
    let registry = crate::test_project_runtime::registry(fixture.directory.path()).unwrap();
    let restored = registry.existing_project(&fixture.project).await;
    assert!(
        matches!(&restored, Ok(Some(_))),
        "cold project reopen failed: {:?}",
        restored.as_ref().err()
    );
    fixture.admission = Arc::new(
        crate::ProjectAdmission::new(crate::ProjectAdmissionProps {
            registry: registry.clone(),
            fleet: fixture.admission.fleet.clone(),
            identity: collaboration_contracts::MachineIdentity::new(
                SERVICE_ID.to_owned().try_into().unwrap(),
                Some("Owner-test-machine"),
            )
            .unwrap(),
        })
        .unwrap(),
    );
    fixture.registry = registry.clone();
    *fixture.store.lock().await =
        crate::SubscriptionWindowStore::new(registry, fixture.interactions.clone());
    fixture
}
