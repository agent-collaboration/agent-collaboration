//! Membership cancellation starts with the real frozen follow and its replicated cause.
use super::native_follow_mail_proofs::{follow_application, frozen_follow};
use super::*;
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::NotificationSource;
use message_board::{
    CancelExpansionSeq, EndReason, HandoffSeq, SubscriptionScope,
    ThreadSubscriptionUnsubscribeRequest,
};
use message_board_storage::{MailCancelGroupCause, MailHandoffItem};

#[tokio::test]
async fn a_resolved_membership_keeps_its_held_item_and_drains_the_unallocated_range() {
    let fixture = OwnerFixture::new().await;
    fixture
        .policy(
            &fixture.root,
            message_board::SubscriptionMode::Deliver,
            message_board::WhenIdle::Hold,
            10,
            30,
        )
        .await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    let first_position = fixture
        .post(&fixture.root, "the already allocated resolved batch")
        .await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture.clock.advance_without_tokio_time(10);
    let target = target_session(&fixture.reader).unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if !native
                .interactions
                .list_held_mail_push_records(&target)
                .await
                .unwrap()
                .is_empty()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let first = frozen_follow(&fixture).await;
    assert_eq!(first.len(), 1);
    let original_item = serde_json::to_value(&first[0]).unwrap();
    fixture.clock.advance_without_tokio_time(1);
    let second_position = fixture
        .post(
            &fixture.root,
            "this owed range must still drain after resolution",
        )
        .await;
    runtime.owner_barrier(&fixture.reader).await;
    assert_eq!(
        frozen_follow(&fixture).await.len(),
        1,
        "the second quiet window was not held for resolution"
    );
    fixture
        .resolve(
            message_board::ThreadResolveRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                reason: "native drain proof".to_owned().try_into().unwrap(),
                thread: fixture.root.clone().into(),
                actor: human("board-owner"),
                acting_for: None,
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.clock.advance_without_tokio_time(30);
    let mut received = std::collections::BTreeSet::new();
    let mut timed_out = false;
    for completed in 1..=2 {
        let request = tokio::time::timeout(Duration::from_secs(6), async {
            runtime.requests.lock().await.recv().await.unwrap()
        })
        .await;
        let Ok(request) = request else {
            timed_out = true;
            break;
        };
        received.insert(request.payload.push_id.as_str().to_owned());
        runtime.completions.send(DeliveryOutcome::Started).unwrap();
        await_native_completion(&runtime, completed).await;
    }
    let frozen = frozen_follow(&fixture).await;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let current = read
        .membership(
            &fixture.reader,
            &SubscriptionScope::thread(fixture.root.clone()),
        )
        .await
        .unwrap()
        .unwrap();
    let cancelled = read.mail_handoff_page(HandoffSeq::ZERO, 100).await.unwrap().into_iter().any(|handoff| matches!(handoff.item,
        MailHandoffItem::CancelGroup(group) if group.membership.as_str() == current.envelope.id.as_str()
            && group.activated_at == current.activated_at() && group.generation == current.generation()
    ));
    read.finish().await.unwrap();
    let public_subscription = fixture
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
    if !timed_out {
        for item in &frozen {
            await_compaction(
                &native,
                DeliveryItemRef {
                    item: item.id(),
                    digest: item.digest().clone(),
                },
            )
            .await;
        }
        tokio::time::timeout(Duration::from_secs(6), runtime.observe(|event| matches!(event,
            crate::service_identity::subscription_delivery::subscription_service::OwnerObservation::Stopped
        ))).await.unwrap();
    }
    let mut outcomes = Vec::new();
    for item in &frozen {
        let push = item.id().as_uuid().to_string().try_into().unwrap();
        outcomes.push(
            native
                .interactions
                .get_push_record(&push)
                .await
                .unwrap()
                .unwrap(),
        );
    }
    publisher.shutdown().await;
    drop(publisher);
    runtime.service.shutdown().await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    assert!(
        !timed_out,
        "resolution cancelled or lost the held batch or its owed drain: {outcomes:?}"
    );
    assert!(
        outcomes
            .iter()
            .all(|record| record.delivery_state == PushDeliveryState::Delivered)
    );
    assert!(!cancelled, "a Resolved membership created a cancel group");
    assert_eq!(current.ended().unwrap().reason, EndReason::Resolved);
    assert_eq!(
        public_subscription.state().end_reason(),
        Some(EndReason::Resolved)
    );
    assert_eq!(frozen.len(), 2);
    assert_eq!(serde_json::to_value(&frozen[0]).unwrap(), original_item);
    assert_eq!(
        received,
        frozen
            .iter()
            .map(|item| item.id().as_uuid().to_string())
            .collect()
    );
    for (index, position) in [first_position, second_position].into_iter().enumerate() {
        let BoardDeliveryBody::Notification(body) = frozen[index].body() else {
            panic!("notification");
        };
        let NotificationSource::FollowBatch {
            roots, draining, ..
        } = &body.source
        else {
            panic!("follow");
        };
        assert_eq!(*draining, index == 1);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].from(), position);
        assert_eq!(roots[0].through(), position);
        assert_eq!(roots[0].post_count(), 1);
    }
}

#[tokio::test]
async fn a_real_membership_end_prevents_its_held_follow_through_a_frozen_child() {
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    fixture
        .post(
            &fixture.root,
            "follow that the real membership end must cancel",
        )
        .await;
    let target = target_session(&fixture.reader).unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if native
                .interactions
                .list_held_mail_push_records(&target)
                .await
                .unwrap()
                .iter()
                .any(|record| record.kind == PushKind::SubscriptionActivity)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let frozen = frozen_follow(&fixture).await;
    assert_eq!(frozen.len(), 1);
    let push_id: collaboration_protocol::PushId =
        frozen[0].id().as_uuid().to_string().try_into().unwrap();
    let show = PushRecordShowParams {
        caller: target,
        reference: push_id.as_str().to_owned(),
    };
    let before = app.messages().push_show(show.clone()).await.unwrap();
    assert_eq!(before.record.delivery_state, PushDeliveryState::Held);
    assert_eq!(before.activity_ranges.len(), 1);
    assert_eq!(
        before.activity_ranges[0].posts[0].text.as_str(),
        "follow that the real membership end must cancel"
    );
    let original = serde_json::to_value(&frozen[0]).unwrap();
    let BoardDeliveryBody::Notification(body) = frozen[0].body() else {
        panic!("notification");
    };
    let NotificationSource::FollowBatch {
        membership,
        activated_at,
        generation,
        ..
    } = &body.source
    else {
        panic!("follow");
    };

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
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let member = read
        .membership(
            &fixture.reader,
            &SubscriptionScope::thread(fixture.root.clone()),
        )
        .await
        .unwrap()
        .unwrap();
    let handoffs = read.mail_handoff_page(HandoffSeq::ZERO, 100).await.unwrap();
    read.finish().await.unwrap();
    let mut original_sequence = None;
    let mut groups = Vec::new();
    for handoff in handoffs {
        match handoff.item {
            MailHandoffItem::Delivery(envelope) if envelope.id() == frozen[0].id() => {
                original_sequence = Some(handoff.sequence)
            }
            MailHandoffItem::CancelGroup(group)
                if &group.membership == membership
                    && group.activated_at == *activated_at
                    && group.generation == *generation =>
            {
                groups.push((handoff.sequence, group));
            }
            _ => {}
        }
    }
    // Local controls need no central ACK (840). Readiness reaches the real
    // pre-injection publisher check; no fixture releases custody or fabricates outcomes.
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.clock.advance_without_tokio_time(30);
    let settled = async {
        loop {
            let outcome = native
                .runtime()
                .local_outcome_for_test(frozen[0].id())
                .await
                .unwrap();
            if let Some(outcome) = outcome
                && !matches!(outcome, fleet_sync_protocol::RecipientOutcome::Held)
            {
                break outcome;
            }
            tokio::task::yield_now().await;
        }
    };
    let mut native_called = false;
    let outcome = tokio::time::timeout(Duration::from_secs(6), async {
        tokio::pin!(settled);
        tokio::select! {
            outcome = &mut settled => outcome,
            request = async { runtime.requests.lock().await.recv().await.unwrap() } => {
                assert_eq!(request.payload.push_id, push_id);
                native_called = true;
                runtime.completions.send(DeliveryOutcome::Started).unwrap();
                settled.await
            }
        }
    })
    .await;
    let mut read = store.snapshot().await.unwrap();
    let expansions = read
        .mail_cancel_expansion_page(CancelExpansionSeq::ZERO, 100)
        .await
        .unwrap();
    read.finish().await.unwrap();
    let mut child_outcomes = Vec::new();
    for child in &expansions {
        child_outcomes.push(
            native
                .runtime()
                .local_outcome_for_test(child.expansion.envelope.id())
                .await
                .unwrap(),
        );
    }
    let after = app.messages().push_show(show).await.unwrap();
    let final_frozen = frozen_follow(&fixture).await;
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    assert!(
        !native_called,
        "membership end allowed a native client call"
    );
    let [child] = expansions.as_slice() else {
        panic!("one durable frozen child per group and item");
    };
    assert!(child.sequence > CancelExpansionSeq::ZERO);
    assert_eq!(child.expansion.target_item.as_str(), push_id.as_str());
    assert!(
        matches!(outcome.expect("membership end never reached SDK settlement"),
        fleet_sync_protocol::RecipientOutcome::Prevented { cancel } if cancel == child.expansion.envelope.id())
    );
    assert!(matches!(child_outcomes.as_slice(),
        [Some(fleet_sync_protocol::RecipientOutcome::Prevented { cancel })] if *cancel == child.expansion.envelope.id()));
    assert_eq!(
        after.record.delivery_state,
        PushDeliveryState::Rejected,
        "SDK Prevented must be projected into the public native record"
    );
    assert!(
        matches!(after.record.last_outcome.as_ref().map(|receipt| &receipt.outcome),
        Some(DeliveryOutcome::NotSubmitted { retryable: false, reason }) if reason == "mail cancelled before local delivery")
    );
    assert_eq!(after.link, before.link);
    assert_eq!(
        serde_json::to_value(&after.activity_ranges).unwrap(),
        serde_json::to_value(&before.activity_ranges).unwrap()
    );
    assert_eq!(final_frozen.len(), 1);
    assert_eq!(serde_json::to_value(&final_frozen[0]).unwrap(), original);
    assert_eq!(member.ended().unwrap().reason, EndReason::Cancelled);
    assert_eq!(
        groups.len(),
        1,
        "membership end committed without its causal cancel group"
    );
    let (sequence, group) = &groups[0];
    assert_eq!(child.expansion.group_handoff, group.id);
    assert!(*sequence > original_sequence.unwrap());
    assert_eq!(&group.membership, membership);
    assert_eq!(group.activated_at, *activated_at);
    assert_eq!(group.generation, *generation);
    assert_eq!(
        group.cause,
        MailCancelGroupCause::MembershipEnded {
            reason: EndReason::Cancelled
        }
    );
}
