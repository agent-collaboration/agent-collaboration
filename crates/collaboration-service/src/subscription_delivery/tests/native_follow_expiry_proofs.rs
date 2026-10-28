//! Expiry comes from the existing reader cadence and a locked project transition.
use super::native_follow_mail_proofs::{follow_application, frozen_follow};
use super::*;
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::NotificationSource;
use message_board::{EndReason, HandoffSeq, RecordedActor, RouterComponent, SubscriptionScope};
use message_board_storage::{MailCancelGroupCause, MailHandoffItem};

#[tokio::test]
async fn startup_expires_an_allocated_follow_even_without_local_custody_or_live_public_rows() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let position = fixture
        .post(
            &fixture.root,
            "allocated before the owner stopped, never queued",
        )
        .await;
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
    read.finish().await.unwrap();
    assert!(
        fixture
            .admission
            .allocate_follow_batch(
                &fixture.project,
                crate::project_admission::FollowBatchProposal {
                    membership: member.envelope.id.clone(),
                    activated_at: member.activated_at(),
                    generation: member.generation(),
                    roots: vec![
                        collaboration_mail_protocol::RootRange::new(
                            fixture.root.clone(),
                            position,
                            position,
                            1
                        )
                        .unwrap()
                    ],
                    item: fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7()).unwrap(),
                    preventions: Vec::new(),
                    enrollment: crate::IntentRecipientEnrollment {
                        service: SERVICE_ID.to_owned().try_into().unwrap(),
                        peer: PeerId::new(
                            uuid::Uuid::parse_str("0192a1b2-c3d4-7000-8000-000000000001").unwrap()
                        )
                        .unwrap(),
                    },
                },
                fixture.clock.now()
            )
            .await
            .unwrap()
            .is_some()
    );
    fixture.clock.advance_without_tokio_time(
        u64::try_from((member.expires_at() - fixture.clock.now()).num_seconds()).unwrap() + 1,
    );
    let runtime = fixture.prepared_runtime();
    // Compose the same weak ports before startup, as the Host does.
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    runtime.service.start().await.unwrap();
    let mut read = store.snapshot().await.unwrap();
    let current = read
        .membership(
            &fixture.reader,
            &SubscriptionScope::thread(fixture.root.clone()),
        )
        .await
        .unwrap()
        .unwrap();
    let groups = read.mail_handoff_page(HandoffSeq::ZERO, 100).await.unwrap().into_iter().filter(|handoff| matches!(&handoff.item,
        MailHandoffItem::CancelGroup(group) if group.membership.as_str() == member.envelope.id.as_str()
            && group.activated_at == member.activated_at() && group.generation == member.generation()
            && group.cause == MailCancelGroupCause::MembershipEnded { reason: EndReason::Expired }
    )).count();
    read.finish().await.unwrap();
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    assert_eq!(
        current.ended().map(|end| end.reason),
        Some(EndReason::Expired),
        "startup filtered out the expired reader before ending its allocated interest"
    );
    assert_eq!(groups, 1);
}

#[tokio::test]
async fn the_existing_reader_expiry_cadence_commits_one_router_end_and_cancel_group() {
    Box::pin(prove_reader_expiry(false)).await;
}

#[tokio::test]
async fn crossing_expiry_during_the_membership_read_keeps_the_reader_deadline() {
    Box::pin(prove_reader_expiry(true)).await;
}

#[tokio::test]
async fn a_renewed_held_follow_delivers_past_its_original_membership_expiry() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    fixture
        .post(
            &fixture.root,
            "renew the membership without replacing its held item",
        )
        .await;
    let target = target_session(&fixture.reader).unwrap();
    let held = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if let Some(held) = native
                .interactions
                .list_held_mail_push_records(&target)
                .await
                .unwrap()
                .into_iter()
                .next()
            {
                break held;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let original = read
        .membership(
            &fixture.reader,
            &SubscriptionScope::thread(fixture.root.clone()),
        )
        .await
        .unwrap()
        .unwrap();
    read.finish().await.unwrap();
    let frozen = frozen_follow(&fixture).await;
    assert_eq!(frozen.len(), 1);
    let original_item = serde_json::to_value(&frozen[0]).unwrap();
    let before = app
        .messages()
        .push_show(PushRecordShowParams {
            caller: target.clone(),
            reference: held.push_id.as_str().to_owned(),
        })
        .await
        .unwrap();
    let until_expiry =
        u64::try_from((original.expires_at() - fixture.clock.now()).num_seconds()).unwrap();
    fixture.clock.advance_without_tokio_time(until_expiry - 1);
    fixture
        .write_as(
            fixture.reader.clone(),
            message_board::BoardOperation::ThreadRenew {
                scope: SubscriptionScope::thread(fixture.root.clone()),
            },
            fixture.clock.now(),
        )
        .await;
    fixture.clock.advance_without_tokio_time(2);
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.clock.advance_without_tokio_time(30);
    let request = tokio::time::timeout(Duration::from_secs(6), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .expect("renewed held follow did not reach its original native client");
    assert_eq!(request.payload.push_id, held.push_id);
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    await_compaction(
        &native,
        DeliveryItemRef {
            item: frozen[0].id(),
            digest: frozen[0].digest().clone(),
        },
    )
    .await;
    let shown = app
        .messages()
        .push_show(PushRecordShowParams {
            caller: target,
            reference: held.push_id.as_str().to_owned(),
        })
        .await
        .unwrap();
    let current_frozen = frozen_follow(&fixture).await;
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
        MailHandoffItem::CancelGroup(group) if group.membership.as_str() == original.envelope.id.as_str()
            && group.activated_at == original.activated_at() && group.generation == original.generation()
    ));
    read.finish().await.unwrap();
    publisher.shutdown().await;
    drop(publisher);
    runtime.service.shutdown().await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    assert!(
        !cancelled,
        "renewal of held work created a membership cancel"
    );
    assert!(current.ended().is_none());
    assert!(current.expires_at() > original.expires_at());
    assert_eq!(shown.record.delivery_state, PushDeliveryState::Delivered);
    assert_eq!(shown.link, before.link);
    assert_eq!(current_frozen.len(), 1);
    assert_eq!(
        serde_json::to_value(&current_frozen[0]).unwrap(),
        original_item
    );
}

async fn prove_reader_expiry(cross_read: bool) {
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    fixture
        .post(&fixture.root, "held through the membership expiry boundary")
        .await;
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
    tokio::time::timeout(Duration::from_secs(6), runtime.observe(|event| matches!(event, crate::subscription_delivery::subscription_service::OwnerObservation::ReconcileRequested))).await.unwrap();
    runtime.owner_barrier(&fixture.reader).await;
    let frozen = frozen_follow(&fixture).await;
    assert_eq!(frozen.len(), 1);
    assert_eq!(
        frozen[0].valid_until(),
        None,
        "renewal must not be replaced by a frozen membership deadline"
    );
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
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let original = read
        .membership(
            &fixture.reader,
            &SubscriptionScope::thread(fixture.root.clone()),
        )
        .await
        .unwrap()
        .unwrap();
    read.finish().await.unwrap();
    let seconds =
        u64::try_from((original.expires_at() - fixture.clock.now()).num_seconds()).unwrap() + 1;
    let probe = if cross_read {
        let probe = fixture
            .store
            .lock()
            .await
            .pause_next_expiry_read_for_test()
            .await;
        runtime.reconcile(&fixture.reader).await;
        tokio::time::timeout(Duration::from_secs(6), probe.wait_for_read())
            .await
            .unwrap();
        Some(probe)
    } else {
        None
    };
    fixture.clock.advance_without_tokio_time(seconds);
    if let Some(probe) = probe {
        probe.release();
    }
    let ended = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let mut read = store.snapshot().await.unwrap();
            let current = read
                .membership(
                    &fixture.reader,
                    &SubscriptionScope::thread(fixture.root.clone()),
                )
                .await
                .unwrap()
                .unwrap();
            read.finish().await.unwrap();
            if current.ended().is_some() {
                break current;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let mut read = store.snapshot().await.unwrap();
    let groups: Vec<_> = read
        .mail_handoff_page(HandoffSeq::ZERO, 100)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|handoff| match handoff.item {
            MailHandoffItem::CancelGroup(group)
                if &group.membership == membership
                    && group.activated_at == *activated_at
                    && group.generation == *generation =>
            {
                Some(group)
            }
            _ => None,
        })
        .collect();
    read.finish().await.unwrap();
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    let ended = ended.expect("reader expiry cadence never committed the membership end");
    assert_eq!(ended.reader(), original.reader());
    assert!(matches!(
        ended.envelope.actor,
        RecordedActor::Router {
            component: RouterComponent::SubscriptionDelivery,
            ..
        }
    ));
    assert_eq!(ended.ended().unwrap().reason, EndReason::Expired);
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].cause,
        MailCancelGroupCause::MembershipEnded {
            reason: EndReason::Expired
        }
    );
}
