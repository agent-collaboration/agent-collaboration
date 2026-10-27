//! Proposals made before a membership change never authorize a later allocation.
use super::*;
use crate::project_admission::FollowBatchProposal;
use collaboration_mail_protocol::RootRange;
use message_board::{FeedCursor, RecordId, SubscriptionMembership, SubscriptionScope, WhenIdle};

pub(super) async fn current_membership(fixture: &OwnerFixture) -> SubscriptionMembership {
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
    member
}

pub(super) fn proposal(
    fixture: &OwnerFixture,
    member: &SubscriptionMembership,
    position: FeedCursor,
) -> FollowBatchProposal {
    FollowBatchProposal {
        membership: member.envelope.id.clone(),
        activated_at: member.activated_at(),
        generation: member.generation(),
        roots: vec![RootRange::new(fixture.root.clone(), position, position, 1).unwrap()],
        item: fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        preventions: Vec::new(),
        enrollment: crate::IntentRecipientEnrollment {
            service: SERVICE_ID.to_owned().try_into().unwrap(),
            peer: PeerId::new(
                uuid::Uuid::parse_str("0192a1b2-c3d4-7000-8000-000000000001").unwrap(),
            )
            .unwrap(),
        },
    }
}

async fn allocation_count(fixture: &OwnerFixture) -> usize {
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let count = read
        .mail_handoff_page(message_board::HandoffSeq::ZERO, 100)
        .await
        .unwrap()
        .into_iter()
        .filter(|handoff| {
            matches!(
                handoff.item,
                message_board_storage::MailHandoffItem::Delivery(_)
            )
        })
        .count();
    read.finish().await.unwrap();
    count
}

#[tokio::test]
async fn follow_admission_rechecks_generation_and_activation_before_allocating() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let position = fixture
        .post(&fixture.root, "owed before the membership changes")
        .await;
    let member = current_membership(&fixture).await;
    let original = proposal(&fixture, &member, position);
    let mut stale_activation = original.clone();
    stale_activation.activated_at = position;
    assert!(
        fixture
            .admission
            .allocate_follow_batch(&fixture.project, stale_activation, fixture.clock.now())
            .await
            .unwrap()
            .is_none()
    );
    fixture
        .policy(
            &fixture.root,
            message_board::SubscriptionMode::Deliver,
            WhenIdle::Hold,
            1,
            2,
        )
        .await;
    assert_ne!(
        current_membership(&fixture).await.generation(),
        member.generation()
    );
    let refused = fixture
        .admission
        .allocate_follow_batch(&fixture.project, original, fixture.clock.now())
        .await
        .unwrap();
    assert!(
        refused.is_none(),
        "admission skipped the current generation re-check"
    );
    assert_eq!(allocation_count(&fixture).await, 0);
}

#[tokio::test]
async fn follow_admission_does_not_allocate_a_missing_membership() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let position = fixture
        .post(&fixture.root, "owed activity is not a membership grant")
        .await;
    let member = current_membership(&fixture).await;
    let mut missing = proposal(&fixture, &member, position);
    missing.membership = RecordId::try_from(uuid::Uuid::now_v7().to_string()).unwrap();
    assert!(
        fixture
            .admission
            .allocate_follow_batch(&fixture.project, missing, fixture.clock.now())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(allocation_count(&fixture).await, 0);
}

#[tokio::test]
async fn follow_admission_refuses_overlapping_ranges_within_and_across_handoffs() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let position = fixture
        .post(&fixture.root, "one range is owed exactly once")
        .await;
    let member = current_membership(&fixture).await;
    let original = proposal(&fixture, &member, position);
    let mut internal_overlap = original.clone();
    internal_overlap
        .roots
        .push(internal_overlap.roots[0].clone());
    assert!(
        fixture
            .admission
            .allocate_follow_batch(&fixture.project, internal_overlap, fixture.clock.now())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(allocation_count(&fixture).await, 0);
    assert!(
        fixture
            .admission
            .allocate_follow_batch(&fixture.project, original.clone(), fixture.clock.now())
            .await
            .unwrap()
            .is_some()
    );
    let mut second = original;
    second.item = fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7()).unwrap();
    assert!(
        fixture
            .admission
            .allocate_follow_batch(&fixture.project, second, fixture.clock.now())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(allocation_count(&fixture).await, 1);
}

#[tokio::test]
async fn poll_project_membership_freezes_one_handoff_per_root_and_rechecks_shape() {
    // A single project membership owns three roots. Independent waiter handovers
    // require independent SDK identities, rather than three roots in one item.
    let fixture = Box::pin(OwnerFixture::new()).await;
    let roots = project_poll_roots(&fixture).await;
    for root in &roots {
        fixture.post(root, "independently handed over").await;
    }
    fixture
        .store
        .lock()
        .await
        .rescan_missing_subscription_windows(&fixture.reader, fixture.clock.now())
        .await
        .unwrap();
    let proposed = fixture
        .store
        .lock()
        .await
        .propose_follow_batches(
            &fixture.reader,
            &roots,
            fixture.clock.now(),
            std::slice::from_ref(&fixture.project),
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        proposed.len(),
        3,
        "Poll roots did not receive independent custody"
    );
    let mut inputs = Vec::new();
    for window in proposed {
        assert_eq!(window.roots.len(), 1);
        inputs.push(FollowBatchProposal {
            membership: window.membership,
            activated_at: window.activated_at,
            generation: window.generation,
            roots: window.roots,
            item: fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7()).unwrap(),
            enrollment: proposal(
                &fixture,
                &current_membership(&fixture).await,
                window.activated_at,
            )
            .enrollment,
            preventions: window.preventions,
        });
    }
    let mut combined = inputs[0].clone();
    combined.roots.push(inputs[1].roots[0].clone());
    assert!(
        fixture
            .admission
            .allocate_follow_batch(&fixture.project, combined, fixture.clock.now())
            .await
            .is_err(),
        "the locked Poll policy admitted a multi-root item"
    );
    for input in inputs {
        assert!(
            fixture
                .admission
                .allocate_follow_batch(&fixture.project, input, fixture.clock.now())
                .await
                .unwrap()
                .is_some()
        );
    }
    assert_eq!(allocation_count(&fixture).await, 3);
    for item in super::native_follow_mail_proofs::frozen_follow(&fixture).await {
        let board_sync_protocol::BoardDeliveryBody::Notification(body) = item.body() else {
            panic!("notification");
        };
        let collaboration_mail_protocol::NotificationSource::FollowBatch { policy, roots, .. } =
            &body.source
        else {
            panic!("follow batch");
        };
        assert_eq!(policy.mode(), message_board::SubscriptionMode::Poll);
        assert_eq!(roots.len(), 1);
    }
}

pub(super) async fn project_poll_roots(fixture: &OwnerFixture) -> Vec<message_board::ThreadId> {
    let mut roots = Vec::new();
    for _ in 0..3 {
        let root = message_board::ThreadId::generate();
        fixture
            .create_root(&root, human(&format!("poll-root-{}", root.as_str())))
            .await;
        roots.push(root);
    }
    fixture
        .write_as(
            fixture.reader.clone(),
            message_board::BoardOperation::ProjectWatch {},
            fixture.clock.now(),
        )
        .await;
    fixture
        .subscribe(
            message_board::ThreadSubscriptionSubscribeRequest {
                reader: fixture.reader.clone(),
                scope: SubscriptionScope::project(fixture.project.clone()),
                policy: message_board::SubscriptionPolicyPatch {
                    mode: Some(message_board::SubscriptionMode::Poll),
                    when_idle: Some(WhenIdle::Hold),
                    timing: message_board::SubscriptionTimingPatch {
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
    roots
}
