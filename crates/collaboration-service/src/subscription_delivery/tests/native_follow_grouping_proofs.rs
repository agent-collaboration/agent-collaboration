//! Membership grouping preserves every range while the native owner keeps each frozen identity.
use super::native_follow_mail_proofs::{follow_application, frozen_follow};
use super::*;
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::NotificationSource;
use message_board::{
    BoardOperation, SubscriptionMode, SubscriptionPolicyPatch, SubscriptionScope,
    SubscriptionTimingPatch, ThreadId, ThreadSubscriptionSubscribeRequest, WhenIdle,
};

async fn wait_held(native: &NativeMailOwner, fixture: &OwnerFixture, expected: usize) {
    let target = target_session(&fixture.reader).unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let held = native
                .interactions
                .list_held_mail_push_records(&target)
                .await
                .unwrap();
            if held
                .iter()
                .filter(|record| record.kind == PushKind::SubscriptionActivity)
                .count()
                == expected
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("native membership follow items were not held");
}

#[tokio::test]
async fn resumed_reader_keeps_separate_membership_items_with_staggered_presence_checks() {
    let fixture = OwnerFixture::new().await;
    let second_root = fixture.add_root().await;
    let poll_root = fixture.add_root().await;
    fixture
        .policy(&poll_root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
        .await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    fixture.post(&fixture.root, "first membership held").await;
    wait_held(&native, &fixture, 1).await;
    runtime.owner_barrier(&fixture.reader).await;
    fixture.clock.advance_without_tokio_time(1);
    fixture.post(&second_root, "second membership held").await;
    wait_held(&native, &fixture, 2).await;
    runtime.owner_barrier(&fixture.reader).await;
    let poll_position = fixture
        .post(&poll_root, "poll activity remains pending")
        .await;
    runtime.owner_barrier(&fixture.reader).await;
    // Preserve the staggered presence schedule: observe publication without
    // issuing an explicit reconcile that would clear its existing checks.
    let frozen = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let items = frozen_follow(&fixture).await;
            if items.len() >= 3 {
                break items;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the Poll post reaches its actual frozen handoff");
    assert_eq!(frozen.len(), 3);
    let (deliver_items, poll_items): (Vec<_>, Vec<_>) = frozen.iter().partition(|item| {
        matches!(item.body(), BoardDeliveryBody::Notification(body)
            if matches!(&body.source, NotificationSource::FollowBatch { policy, .. }
                if policy.mode() == SubscriptionMode::Deliver))
    });
    assert_eq!(deliver_items.len(), 2);
    assert_eq!(poll_items.len(), 1);
    let BoardDeliveryBody::Notification(poll_body) = poll_items[0].body() else {
        panic!("Poll notification");
    };
    let NotificationSource::FollowBatch { roots, policy, .. } = &poll_body.source else {
        panic!("frozen Poll source");
    };
    assert_eq!(policy.mode(), SubscriptionMode::Poll);
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].root(), &poll_root);
    assert_eq!(roots[0].from(), poll_position);
    assert_eq!(roots[0].through(), poll_position);
    assert_eq!(roots[0].post_count(), 1);
    let mut memberships = std::collections::BTreeSet::new();
    let mut frozen_roots = std::collections::BTreeSet::new();
    for item in &deliver_items {
        let BoardDeliveryBody::Notification(body) = item.body() else {
            panic!("notification");
        };
        let NotificationSource::FollowBatch {
            membership, roots, ..
        } = &body.source
        else {
            panic!("follow");
        };
        memberships.insert(membership.as_str().to_owned());
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].post_count(), 1);
        frozen_roots.insert(roots[0].root().clone());
    }
    assert_eq!(
        memberships.len(),
        2,
        "different memberships were coalesced into one item"
    );
    assert_eq!(
        frozen_roots,
        [fixture.root.clone(), second_root].into_iter().collect()
    );

    // The first item's existing presence deadline elapses one second before the second's.
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.clock.advance_without_tokio_time(29);
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
    assert!(matches!(
        native
            .runtime()
            .local_outcome_for_test(poll_items[0].id())
            .await
            .unwrap(),
        Some(fleet_sync_protocol::RecipientOutcome::Held)
    ));
    publisher.shutdown().await;
    drop(publisher);
    runtime.service.shutdown().await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    assert!(
        !timed_out,
        "a running reader left its second membership behind a staggered presence deadline"
    );
    assert_eq!(
        received,
        deliver_items
            .iter()
            .map(|item| item.id().as_uuid().to_string())
            .collect()
    );
}

#[tokio::test]
async fn one_membership_drains_twenty_roots_then_the_remaining_root_without_lost_ranges() {
    let fixture = OwnerFixture::new().await;
    let mut roots = Vec::new();
    for _ in 0..21 {
        let root = ThreadId::generate();
        fixture
            .create_root(&root, human(&format!("project-root-{}", root.as_str())))
            .await;
        roots.push(root);
    }
    fixture
        .write_as(
            fixture.reader.clone(),
            BoardOperation::ProjectWatch {},
            fixture.clock.now(),
        )
        .await;
    fixture
        .subscribe(
            ThreadSubscriptionSubscribeRequest {
                reader: fixture.reader.clone(),
                scope: SubscriptionScope::project(fixture.project.clone()),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Deliver),
                    when_idle: Some(WhenIdle::Hold),
                    timing: SubscriptionTimingPatch {
                        quiet_seconds: Some(0),
                        cap_seconds: Some(0),
                    },
                    lifetime_seconds: None,
                },
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    for root in &roots {
        fixture
            .post(root, "one project membership owns this range")
            .await;
    }
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    runtime
        .service
        .reconcile_reader(fixture.reader.clone())
        .await
        .unwrap();
    let mut received = std::collections::BTreeSet::new();
    for completed in 1..=2 {
        let request = tokio::time::timeout(Duration::from_secs(6), async {
            runtime.requests.lock().await.recv().await.unwrap()
        })
        .await
        .expect("remaining due roots did not drain immediately");
        received.insert(request.payload.push_id.as_str().to_owned());
        runtime.completions.send(DeliveryOutcome::Started).unwrap();
        await_native_completion(&runtime, completed).await;
    }
    runtime.owner_barrier(&fixture.reader).await;
    let frozen = frozen_follow(&fixture).await;
    assert_eq!(frozen.len(), 2);
    let mut group = None;
    let mut counts = Vec::new();
    let mut allocated_roots = std::collections::BTreeSet::new();
    for item in &frozen {
        let BoardDeliveryBody::Notification(body) = item.body() else {
            panic!("notification");
        };
        let NotificationSource::FollowBatch {
            membership,
            activated_at,
            generation,
            roots,
            ..
        } = &body.source
        else {
            panic!("follow");
        };
        let current = (membership.clone(), *activated_at, *generation);
        assert_eq!(group.get_or_insert_with(|| current.clone()), &current);
        counts.push(roots.len());
        for root in roots {
            assert!(
                allocated_roots.insert(root.root().clone()),
                "a root range was allocated twice"
            );
            assert_eq!(root.post_count(), 1);
        }
        await_compaction(
            &native,
            DeliveryItemRef {
                item: item.id(),
                digest: item.digest().clone(),
            },
        )
        .await;
    }
    assert_eq!(counts, vec![20, 1]);
    assert_eq!(allocated_roots, roots.into_iter().collect());
    assert_eq!(
        received,
        frozen
            .iter()
            .map(|item| item.id().as_uuid().to_string())
            .collect()
    );
    publisher.shutdown().await;
    drop(publisher);
    runtime.service.shutdown().await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}
