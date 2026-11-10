use super::super::{
    SubscriptionWaitFilter, SubscriptionWaitResult, subscription_service::OwnerObservation,
};
use super::owner_fixture::*;
use message_board::*;
use router_clock::RuntimeClock;
use sqlx::Connection;

#[tokio::test]
async fn concurrent_session_waits_are_fifo_and_never_receive_the_same_range() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Poll,
            WhenIdle::Hold,
            0,
            900,
        )
        .await;
    let runtime = fixture.native_runtime().await;
    let first_service = runtime.service.clone();
    let first_reader = fixture.reader.clone();
    let topic = fixture.project.clone();
    let first = tokio::spawn(async move {
        first_service
            .wait(
                first_reader,
                SubscriptionWaitFilter::Project(topic),
                60,
                usize::MAX,
            )
            .await
            .unwrap()
            .unwrap()
    });
    runtime
        .observe(|event| matches!(event, OwnerObservation::WaitQueued))
        .await;
    let second_service = runtime.service.clone();
    let second_reader = fixture.reader.clone();
    let second = tokio::spawn(async move {
        second_service
            .wait(second_reader, SubscriptionWaitFilter::All, 60, usize::MAX)
            .await
            .unwrap()
            .unwrap()
    });
    runtime
        .observe(|event| matches!(event, OwnerObservation::WaitQueued))
        .await;
    let first_post = fixture.post(&fixture.root, "First range").await;
    let SubscriptionWaitResult::Notice {
        push_id: first_id,
        batch: first_batch,
        line,
    } = first.await.unwrap()
    else {
        panic!("session notice");
    };
    assert!(line.as_str().starts_with("🧵 Router: new thread activity"));
    assert_eq!(first_batch.roots[0].through, first_post);
    fixture
        .await_push_state(
            &first_id,
            collaboration_protocol::PushDeliveryState::Delivered,
        )
        .await;
    assert!(!second.is_finished());
    let second_post = fixture.post(&fixture.root, "Second range").await;
    let SubscriptionWaitResult::Notice {
        push_id: second_id,
        batch: second_batch,
        ..
    } = second.await.unwrap()
    else {
        panic!("session notice");
    };
    assert_ne!(first_id, second_id);
    assert_eq!(second_batch.roots[0].from, second_post);
    fixture
        .await_push_state(
            &second_id,
            collaboration_protocol::PushDeliveryState::Delivered,
        )
        .await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
}

#[tokio::test]
async fn human_poll_returns_bodyless_ranges_without_storing_or_pushing() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture
        .policy(&fixture.root, SubscriptionMode::Off, WhenIdle::Hold, 0, 900)
        .await;
    let reader = human("human-poll-reader");
    {
        fixture
            .join_request(
                ThreadJoinRequest {
                    request_id: message_board::RequestId::generate(),
                    protocol_version: message_board::ProtocolVersion::CURRENT,
                    mode: None,
                    when_idle: None,
                    thread: (fixture.root.clone()).into(),
                    actor: reader.clone(),
                    role: ParticipantRole::Participant,
                    watch: true,
                    replace: None,
                    note: None,
                },
                fixture.clock.utc_now(),
            )
            .await
            .unwrap();
        fixture
            .subscribe(
                ThreadSubscriptionSubscribeRequest {
                    reader: reader.clone(),
                    scope: SubscriptionScope::thread(fixture.root.clone()),
                    policy: SubscriptionPolicyPatch {
                        mode: Some(SubscriptionMode::Poll),
                        when_idle: None,
                        timing: SubscriptionTimingPatch {
                            quiet_seconds: Some(0),
                            cap_seconds: Some(900),
                        },
                        lifetime_seconds: None,
                    },
                },
                fixture.clock.utc_now(),
            )
            .await
            .unwrap();
    }
    let runtime = fixture.runtime().await;
    let service = runtime.service.clone();
    let waiter = tokio::spawn(async move {
        service
            .wait(reader, SubscriptionWaitFilter::All, 60, usize::MAX)
            .await
            .unwrap()
            .unwrap()
    });
    runtime
        .observe(|event| matches!(event, OwnerObservation::WaitQueued))
        .await;
    fixture
        .post(&fixture.root, "Human-visible source remains on the board")
        .await;
    let SubscriptionWaitResult::Ranges { batch } = waiter.await.unwrap() else {
        panic!("human gets ranges");
    };
    assert_eq!(batch.roots[0].post_count, 1);
    runtime
        .observe(|event| matches!(event, OwnerObservation::Settled))
        .await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM router_pushes")
        .fetch_one(&mut observer)
        .await
        .unwrap();
    assert_eq!(stored, 0);
    observer.close().await.unwrap();
    runtime.close().await;
}

#[tokio::test]
async fn poll_wait_timeout_is_empty_and_non_poll_wait_is_rejected() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.runtime().await;
    assert!(
        runtime
            .service
            .wait(
                fixture.reader.clone(),
                SubscriptionWaitFilter::All,
                1,
                usize::MAX,
            )
            .await
            .is_err()
    );
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Poll,
            WhenIdle::Hold,
            0,
            900,
        )
        .await;
    runtime
        .service
        .reconcile_reader(fixture.reader.clone())
        .await
        .unwrap();
    let service = runtime.service.clone();
    let reader = fixture.reader.clone();
    let waiter = tokio::spawn(async move {
        service
            .wait(reader, SubscriptionWaitFilter::All, 60, usize::MAX)
            .await
            .unwrap()
    });
    runtime
        .observe(|event| matches!(event, OwnerObservation::WaitQueued))
        .await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture
        .clock
        .advance(std::time::Duration::from_secs(59))
        .unwrap();
    runtime.owner_barrier(&fixture.reader).await;
    assert!(!waiter.is_finished());
    fixture
        .clock
        .advance(std::time::Duration::from_secs(1))
        .unwrap();
    assert!(waiter.await.unwrap().is_none());
    runtime.close().await;
}

#[tokio::test]
async fn dropping_the_last_service_handle_cancels_its_owned_tasks() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.runtime().await;
    runtime.owner_barrier(&fixture.reader).await;
    let mut events = runtime.service.observe_owners();
    drop(runtime);
    loop {
        if matches!(events.recv().await.unwrap(), OwnerObservation::Stopped) {
            break;
        }
    }
    assert!(
        fixture
            .store
            .lock()
            .await
            .get_thread_subscription_record(
                &fixture.reader,
                &SubscriptionScope::thread(fixture.root.clone()),
                fixture.clock.utc_now()
            )
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn finite_wait_budget_hands_oldest_root_then_preserves_remaining_due_root() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let oldest_root = fixture.root.clone();
    let later_root = fixture.add_root().await;
    for root in [&oldest_root, &later_root] {
        fixture
            .policy(root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 900)
            .await;
    }
    let runtime = fixture.native_runtime().await;
    let oldest_post = fixture.post(&oldest_root, "Oldest pending root").await;
    fixture.post(&later_root, "Later pending root").await;
    let oldest_notice = PendingRootNotice::new(
        oldest_root.clone(),
        fixture.project.clone(),
        oldest_post,
        oldest_post,
        1,
    )
    .unwrap();
    let maximum_root_notice_bytes = serde_json::to_vec(std::slice::from_ref(&oldest_notice))
        .unwrap()
        .len();

    let first_result = (runtime.service.wait(
        fixture.reader.clone(),
        SubscriptionWaitFilter::All,
        60,
        maximum_root_notice_bytes,
    ))
    .await
    .unwrap()
    .unwrap();
    let SubscriptionWaitResult::Notice {
        push_id: first_id,
        batch: first_batch,
        ..
    } = first_result
    else {
        panic!("session wait returns a stored notice");
    };
    assert_eq!(first_batch.roots.len(), 1);
    assert_eq!(first_batch.roots[0].thread_id, oldest_root);
    assert!(serde_json::to_vec(&first_batch.roots).unwrap().len() <= maximum_root_notice_bytes);
    fixture
        .await_push_state(
            &first_id,
            collaboration_protocol::PushDeliveryState::Delivered,
        )
        .await;
    // First delivery does not join publication of the later root. Observe that
    // root's frozen item reaching SDK custody before checking its pending state.
    let target = super::super::subscription_push::target_session(&fixture.reader).unwrap();
    let still_pending = (async {
        loop {
            let frozen =
                super::local_mail_proofs::native_follow_mail_proofs::frozen_follow(&fixture).await;
            let later_item = frozen.iter().find(|item| {
                matches!(item.body(), board_sync_protocol::BoardDeliveryBody::Notification(body)
                    if matches!(&body.source, collaboration_mail_protocol::NotificationSource::FollowBatch { roots, .. }
                        if roots.len() == 1 && roots[0].root() == &later_root))
            });
            if let Some(later_item) = later_item {
                let pending = fixture
                    .interactions
                    .pending_poll_mail(&target)
                    .await
                    .unwrap();
                if pending.iter().any(|row| row.item == later_item.id()) {
                    break pending;
                }
            }
            tokio::task::yield_now().await;
        }
    }).await;
    assert_eq!(still_pending.len(), 1);
    let pending_push: collaboration_protocol::PushId = still_pending[0]
        .item
        .as_uuid()
        .to_string()
        .try_into()
        .unwrap();
    assert_ne!(pending_push, first_id);
    assert!(
        fixture
            .interactions
            .get_push_record(&pending_push)
            .await
            .unwrap()
            .is_none()
    );

    let second_result = (runtime.service.wait(
        fixture.reader.clone(),
        SubscriptionWaitFilter::All,
        60,
        maximum_root_notice_bytes,
    ))
    .await
    .unwrap()
    .unwrap();
    let SubscriptionWaitResult::Notice {
        push_id: second_id,
        batch: second_batch,
        ..
    } = second_result
    else {
        panic!("session wait returns a stored notice");
    };
    assert_eq!(second_batch.roots.len(), 1);
    assert_eq!(second_batch.roots[0].thread_id, later_root);
    assert_ne!(first_id, second_id);
    assert_eq!(second_id, pending_push);
    assert!(serde_json::to_vec(&second_batch.roots).unwrap().len() <= maximum_root_notice_bytes);
    fixture
        .await_push_state(
            &second_id,
            collaboration_protocol::PushDeliveryState::Delivered,
        )
        .await;
    assert!(
        fixture
            .interactions
            .pending_poll_mail(&target)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        fixture
            .store
            .lock()
            .await
            .due_subscription_roots(&fixture.reader, fixture.clock.utc_now())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.close().await;
}
