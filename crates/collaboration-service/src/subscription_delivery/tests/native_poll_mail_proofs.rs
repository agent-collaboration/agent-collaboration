//! Poll hands the real SDK item to a waiter, without a native session submission.
use super::native_follow_mail_proofs::{follow_application, frozen_follow};
use super::*;
use crate::{SubscriptionWaitFilter, SubscriptionWaitResult};
use fleet_sync_protocol::RecipientOutcome;
use message_board::{SubscriptionMode, WhenIdle};

#[tokio::test]
async fn a_poll_sdk_item_waits_without_a_push_then_hands_over_its_own_identity_once() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("collaboration_service=trace")
        .with_test_writer()
        .try_init();
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture
        .policy(&fixture.root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
        .await;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    let position = fixture.post(&fixture.root, "SDK-owned Poll debt").await;
    runtime
        .service
        .reconcile_reader(fixture.reader.clone())
        .await
        .unwrap();
    runtime.owner_barrier(&fixture.reader).await;
    let frozen = frozen_follow(&fixture).await;
    assert_eq!(frozen.len(), 1);
    publisher.wait_project_idle_for_test(&fixture.project).await;
    let item = frozen[0].id();
    tokio::time::timeout(Duration::from_secs(6), async {
        while native
            .runtime()
            .local_outcome_for_test(item)
            .await
            .unwrap()
            .is_none()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the Poll item never produced its first actual SDK outcome");
    let push = item.as_uuid().to_string().try_into().unwrap();
    assert!(
        fixture
            .interactions
            .get_push_record(&push)
            .await
            .unwrap()
            .is_none()
    );
    let pending_before = fixture
        .interactions
        .pending_poll_mail(&target_session(&fixture.reader).unwrap())
        .await
        .unwrap();
    let held_before = native.runtime().local_outcome_for_test(item).await.unwrap();
    let pending_view = fixture.public_subscription_view(&runtime).await;

    let service = runtime.service.clone();
    let reader = fixture.reader.clone();
    let mut waiting = tokio::spawn(async move {
        service
            .wait(reader, SubscriptionWaitFilter::All, 60, usize::MAX)
            .await
    });
    let response = tokio::time::timeout(Duration::from_secs(6), &mut waiting).await;
    if response.is_err() {
        waiting.abort();
        let _ = waiting.await;
    }
    let outcome = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let outcome = native.runtime().local_outcome_for_test(item).await.unwrap();
            if !matches!(outcome, None | Some(RecipientOutcome::Held)) {
                break outcome;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let native_submission = runtime.requests.lock().await.try_recv().is_ok();
    let record = fixture.interactions.get_push_record(&push).await.unwrap();
    let pending_after = fixture.interactions.pending_poll_item(item).await.unwrap();
    let shown = if record.is_some() {
        Some(
            app.messages()
                .push_show(PushRecordShowParams {
                    caller: target_session(&fixture.reader).unwrap(),
                    reference: push.as_str().to_owned(),
                })
                .await
                .unwrap(),
        )
    } else {
        None
    };
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;

    assert!(
        matches!(held_before, Some(RecipientOutcome::Held)),
        "no registered waiter must retain SDK Held, observed {held_before:?}"
    );
    assert_eq!(
        pending_before.len(),
        1,
        "SDK receive did not project Poll readiness"
    );
    assert_eq!(pending_before[0].item, item);
    assert_eq!(
        pending_view.pending_count, 1,
        "the public view lost allocated no-push Poll debt"
    );
    assert!(
        matches!(
            outcome,
            Ok(Some(RecipientOutcome::Delivered {
                acceptance: collaboration_mail_protocol::CarrierAcceptance::Started
            }))
        ),
        "Poll hand-over did not settle its SDK item Started: {outcome:?}; response={response:?}; record={record:?}"
    );
    let SubscriptionWaitResult::Notice { push_id, batch, .. } = response
        .expect("an SDK Poll item was stranded instead of handed over")
        .unwrap()
        .unwrap()
        .unwrap()
    else {
        panic!("session Poll requires its own stored notice");
    };
    assert_eq!(push_id, push);
    assert_eq!(batch.roots.len(), 1);
    assert_eq!(batch.roots[0].through, position);
    assert_eq!(batch.roots[0].post_count, 1);
    assert_eq!(record.unwrap().delivery_state, PushDeliveryState::Delivered);
    assert!(
        pending_after.is_none(),
        "terminal SDK outcome retained Poll discovery policy"
    );
    let shown = shown.unwrap();
    assert_eq!(shown.activity_ranges.len(), 1);
    assert_eq!(
        shown.activity_ranges[0].posts[0].text.as_str(),
        "SDK-owned Poll debt"
    );
    assert!(
        !native_submission,
        "Poll invoked the native session carrier"
    );
}

#[tokio::test]
async fn three_poll_roots_keep_filtered_and_fifo_custody_independent() {
    use futures_util::FutureExt;
    let fixture = Box::pin(OwnerFixture::new()).await;
    let roots = super::follow_admission_proofs::project_poll_roots(&fixture).await;
    for root in &roots {
        fixture.post(root, "one independent Poll handover").await;
    }
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    let proof = std::panic::AssertUnwindSafe(async {
        runtime
            .service
            .reconcile_reader(fixture.reader.clone())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(6), async {
            while frozen_follow(&fixture).await.len() != 3 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("three roots did not freeze as three SDK items");
        publisher.wait_project_idle_for_test(&fixture.project).await;
        let frozen = frozen_follow(&fixture).await;
        let target = target_session(&fixture.reader).unwrap();
        let pending = fixture
            .interactions
            .pending_poll_mail(&target)
            .await
            .unwrap();
        assert_eq!(pending.len(), 3);
        assert!(
            pending
                .iter()
                .all(|row| row.membership == pending[0].membership)
        );
        let mut ordered = Vec::new();
        for row in &pending {
            let item = frozen.iter().find(|item| item.id() == row.item).unwrap();
            let board_sync_protocol::BoardDeliveryBody::Notification(body) = item.body() else {
                panic!("notification");
            };
            let collaboration_mail_protocol::NotificationSource::FollowBatch {
                roots, policy, ..
            } = &body.source
            else {
                panic!("follow");
            };
            assert_eq!(policy.mode(), SubscriptionMode::Poll);
            assert_eq!(roots.len(), 1);
            ordered.push((row.item, roots[0].root().clone()));
        }
        for index in 0..3 {
            let filter = if index == 0 {
                SubscriptionWaitFilter::Roots(vec![ordered[0].1.clone().into()])
            } else {
                SubscriptionWaitFilter::All
            };
            let result = tokio::time::timeout(
                Duration::from_secs(6),
                runtime
                    .service
                    .wait(fixture.reader.clone(), filter, 60, usize::MAX),
            )
            .await
            .expect("waiting Poll backlog was not handed over immediately")
            .unwrap()
            .unwrap();
            let SubscriptionWaitResult::Notice { push_id, batch, .. } = result else {
                panic!("session notice");
            };
            assert_eq!(push_id.as_str(), ordered[index].0.as_uuid().to_string());
            assert_eq!(
                batch.roots.len(),
                1,
                "one response merged independent SDK items"
            );
            assert_eq!(batch.roots[0].thread_id, ordered[index].1);
            fixture
                .await_push_state(&push_id, PushDeliveryState::Delivered)
                .await;
            assert!(matches!(
                native
                    .runtime()
                    .local_outcome_for_test(ordered[index].0)
                    .await
                    .unwrap(),
                Some(RecipientOutcome::Delivered {
                    acceptance: collaboration_mail_protocol::CarrierAcceptance::Started
                })
            ));
            let remaining = fixture
                .interactions
                .pending_poll_mail(&target)
                .await
                .unwrap();
            assert_eq!(
                remaining.iter().map(|row| row.item).collect::<Vec<_>>(),
                ordered[index + 1..]
                    .iter()
                    .map(|(item, _)| *item)
                    .collect::<Vec<_>>()
            );
            for (item, _) in &ordered[index + 1..] {
                assert!(matches!(
                    native
                        .runtime()
                        .local_outcome_for_test(*item)
                        .await
                        .unwrap(),
                    Some(RecipientOutcome::Held)
                ));
                let push = item.as_uuid().to_string().try_into().unwrap();
                assert!(
                    fixture
                        .interactions
                        .get_push_record(&push)
                        .await
                        .unwrap()
                        .is_none()
                );
            }
        }
        assert!(
            runtime.requests.lock().await.try_recv().is_err(),
            "Poll invoked a native session carrier"
        );
    })
    .catch_unwind()
    .await;
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    if let Err(failure) = proof {
        std::panic::resume_unwind(failure);
    }
}
