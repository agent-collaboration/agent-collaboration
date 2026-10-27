//! The same membership ID cannot substitute a later activation for frozen debt.
use super::super::follow_admission_proofs::current_membership;
use super::*;
use message_board::*;

async fn assert_reactivated_frontier(resolved: bool) {
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let marks = Arc::new(FrontierMarks(AtomicU64::new(0)));
    let (app, native, publisher) =
        follow_application_with_replication(&fixture, &runtime, None, marks.clone()).await;
    let activation_a = current_membership(&fixture).await;
    let position = fixture
        .post(&fixture.root, "activity belongs to activation A")
        .await;
    fixture.held_native_pushes(1).await;
    let first = frozen_follow(&fixture).await.pop().unwrap();
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let id = first.id().as_uuid().to_string().try_into().unwrap();
    let original_sequence = read.mail_handoff(&id).await.unwrap().unwrap().sequence;
    read.finish().await.unwrap();

    if resolved {
        // Produce a real SDK prevention and a real allocated A replacement,
        // then keep that frozen replacement Held while activation B replaces A.
        fixture
            .policy(
                &fixture.root,
                SubscriptionMode::Deliver,
                WhenIdle::Hold,
                1,
                2,
            )
            .await;
        publisher
            .queue_project(fixture.project.clone())
            .await
            .unwrap();
        fixture.clock.advance_without_tokio_time(2);
        tokio::time::timeout(Duration::from_secs(10), async {
            while frozen_follow(&fixture).await.len() != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("generation replacement must be allocated before reactivation");
        fixture.held_native_pushes(1).await;
    }
    publisher.shutdown().await;
    if resolved {
        fixture
            .resolve(
                ThreadResolveRequest {
                    request_id: RequestId::generate(),
                    protocol_version: ProtocolVersion::CURRENT,
                    thread: fixture.root.clone().into(),
                    actor: human("board-owner"),
                    acting_for: None,
                    reason: "resolved A still owes its frozen range"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                },
                fixture.clock.now(),
            )
            .await
            .unwrap();
        fixture
            .write_as(
                human("board-owner"),
                BoardOperation::ThreadUnresolve {
                    thread_id: fixture.root.clone(),
                },
                fixture.clock.now(),
            )
            .await;
        fixture
            .join_request(
                ThreadJoinRequest {
                    request_id: RequestId::generate(),
                    protocol_version: ProtocolVersion::CURRENT,
                    thread: fixture.root.clone().into(),
                    actor: fixture.reader.clone(),
                    role: ParticipantRole::Participant,
                    watch: true,
                    mode: Some(SubscriptionMode::Deliver),
                    when_idle: Some(WhenIdle::Hold),
                    replace: None,
                    note: None,
                },
                fixture.clock.now(),
            )
            .await
            .unwrap();
    } else {
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
    }
    fixture
        .policy(
            &fixture.root,
            SubscriptionMode::Deliver,
            WhenIdle::Hold,
            1,
            2,
        )
        .await;
    let activation_b = current_membership(&fixture).await;
    assert_eq!(activation_b.envelope.id, activation_a.envelope.id);
    assert_ne!(activation_b.activated_at(), activation_a.activated_at());
    let mut read = store.snapshot().await.unwrap();
    let historical = read
        .membership_activation(&activation_a.envelope.id, activation_a.activated_at())
        .await
        .unwrap()
        .expect("A history survives B's current row");
    assert_eq!(historical.activated_at(), activation_a.activated_at());
    assert_eq!(
        historical.ended().unwrap().reason == EndReason::Resolved,
        resolved
    );
    read.finish().await.unwrap();
    let recovery = Arc::new(crate::NotificationPublisher::new(
        fixture.admission.clone(),
        fixture.interactions.clone(),
        Arc::downgrade(&native.runtime_handle()),
    ));
    recovery
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    recovery.wait_project_idle_for_test(&fixture.project).await;
    if resolved {
        assert!(
            frontier(&fixture).await < original_sequence,
            "activation mismatch was treated as settled despite A's uncovered resolved debt"
        );
        *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
        fixture.clock.advance_without_tokio_time(30);
        let request = tokio::time::timeout(Duration::from_secs(10), async {
            runtime.requests.lock().await.recv().await.unwrap()
        })
        .await
        .unwrap();
        let frozen = frozen_follow(&fixture).await;
        assert_eq!(
            request.payload.push_id.as_str(),
            frozen[1].id().as_uuid().to_string()
        );
        runtime.completions.send(DeliveryOutcome::Started).unwrap();
        await_native_completion(&runtime, 1).await;
        recovery
            .queue_project(fixture.project.clone())
            .await
            .unwrap();
    }
    let mut read = store.snapshot().await.unwrap();
    let head = read.mail_handoff_head().await.unwrap();
    read.finish().await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while frontier(&fixture).await != head {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("historical activation failed to settle the actual handoff head");
    assert!(matches!(
        native
            .runtime()
            .local_outcome_for_test(first.id())
            .await
            .unwrap(),
        Some(fleet_sync_protocol::RecipientOutcome::Prevented { .. })
    ));
    let frozen = frozen_follow(&fixture).await;
    assert_eq!(
        frozen.len(),
        if resolved { 2 } else { 1 },
        "A's activity was reallocated as B"
    );
    for item in &frozen {
        let board_sync_protocol::BoardDeliveryBody::Notification(body) = item.body() else {
            panic!("follow body");
        };
        let collaboration_mail_protocol::NotificationSource::FollowBatch {
            activated_at,
            roots,
            ..
        } = &body.source
        else {
            panic!("follow source");
        };
        assert_eq!(*activated_at, activation_a.activated_at());
        assert_eq!(roots[0].from(), position);
    }
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "unexpected native delivery after reactivation"
    );
    marks.0.store(head.get(), Ordering::SeqCst);
    recovery
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while fixture
            .interactions
            .mail_retired_through(&fixture.project)
            .await
            .unwrap()
            != head
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("acknowledged historical custody was not retired");
    assert!(
        native
            .runtime()
            .local_outcome_for_test(first.id())
            .await
            .unwrap()
            .is_none()
    );
    recovery.shutdown().await;
    drop(recovery);
    drop(publisher);
    drop(app);
    drop(store);
    runtime.close().await;
    let fixture = fixture.reopen_project_and_native(native).await;
    assert_eq!(
        fixture
            .interactions
            .mail_retired_through(&fixture.project)
            .await
            .unwrap(),
        head
    );
    let runtime = fixture.runtime().await;
    let (app, native, publisher) =
        follow_application_with_replication(&fixture, &runtime, None, marks).await;
    publisher
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    publisher.wait_project_idle_for_test(&fixture.project).await;
    assert_eq!(frontier(&fixture).await, head);
    assert!(
        native
            .runtime()
            .local_outcome_for_test(first.id())
            .await
            .unwrap()
            .is_none()
    );
    assert!(runtime.requests.lock().await.try_recv().is_err());
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}

#[tokio::test]
async fn cancelled_activation_settles_after_reactivation_and_acknowledged_reopen() {
    Box::pin(assert_reactivated_frontier(false)).await;
}
#[tokio::test]
async fn resolved_activation_stays_owed_after_reactivation_until_its_frozen_replacement_delivers() {
    Box::pin(assert_reactivated_frontier(true)).await;
}
