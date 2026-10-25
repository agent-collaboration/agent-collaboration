//! Native accepted lines, rather than epoch equality, govern local delivery windows.
use super::follow_admission_proofs::{current_membership, proposal};
use super::window_lineage_fixture::copied_reader;
use super::*;
use interaction_storage::{SubscriptionDeliveryProgress, SubscriptionDeliveryProgressProps};
use message_board::{EpochNumber, FeedCursor, FeedPosition, SubscriptionDeliveryOutcome};
use message_board_storage::CursorValidity;

#[tokio::test]
async fn a_mixed_epoch_window_freezes_round_trips_and_passes_actual_admission() {
    let copied = copied_reader().await;
    let owner = copied.owner;
    let current = owner.post(&owner.root, "new-line followed post").await;
    assert!(copied.retained.epoch < current.epoch);
    assert_eq!(current.position, copied.discarded.position);
    let mut windows = owner.store.lock().await;
    assert_eq!(
        windows
            .rescan_missing_subscription_windows(&owner.reader, owner.clock.now())
            .await
            .unwrap(),
        2
    );
    let mut proposals = windows
        .propose_follow_batches(
            &owner.reader,
            std::slice::from_ref(&owner.root),
            owner.clock.now(),
            std::slice::from_ref(&owner.project),
            &[],
        )
        .await
        .expect("mixed assigning epochs must freeze into one range");
    assert_eq!(proposals.len(), 1);
    let frozen = proposals.pop().unwrap();
    let expected = frozen.roots.clone();
    assert_eq!(expected.len(), 1);
    assert_eq!(expected[0].from(), copied.retained);
    assert_eq!(expected[0].through(), current);
    assert_eq!(expected[0].post_count(), 2);
    let wire = serde_json::to_vec(&expected).unwrap();
    let restored: Vec<collaboration_mail_protocol::RootRange> =
        serde_json::from_slice(&wire).unwrap();
    assert_eq!(restored, expected);
    drop(windows);
    let member = current_membership(&owner).await;
    let mut input = proposal(&owner, &member, current);
    input.roots = restored;
    let allocated = owner
        .admission
        .allocate_follow_batch(&owner.project, input, owner.clock.now())
        .await
        .unwrap();
    assert!(
        allocated.is_some(),
        "actual admission refused a retained e1/e2 post range"
    );
    let frozen = super::native_follow_mail_proofs::frozen_follow(&owner).await;
    assert_eq!(frozen.len(), 1);
    let board_sync_protocol::BoardDeliveryBody::Notification(body) = frozen[0].body() else {
        panic!("follow notification");
    };
    let collaboration_mail_protocol::NotificationSource::FollowBatch { roots, .. } = &body.source
    else {
        panic!("follow source");
    };
    assert_eq!(*roots, expected);
    drop(owner.into_directory());
}

#[tokio::test]
async fn discarded_window_and_selection_resume_without_losing_or_duplicating_current_activity() {
    let copied = copied_reader().await;
    let owner = copied.owner;
    owner
        .interactions
        .put_subscription_progress(&copied.progress)
        .await
        .unwrap();
    let current = owner
        .post(&owner.root, "replacement current-line post")
        .await;
    let store = owner
        .registry
        .existing_project(&owner.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    assert_eq!(
        read.cursor_validity(copied.retained).await.unwrap(),
        CursorValidity::Valid
    );
    assert_eq!(
        read.cursor_validity(copied.discarded).await.unwrap(),
        CursorValidity::Superseded {
            resume_from: copied.boundary
        }
    );
    read.finish().await.unwrap();
    let member = current_membership(&owner).await;
    let mut windows = owner.store.lock().await;
    // A stale receipt clears its discarded selection without settling reused ordinals.
    windows
        .settle_subscription_batch(
            &owner.reader,
            &copied.selection,
            SubscriptionDeliveryOutcome::Accepted,
        )
        .await
        .unwrap();
    let normalized = owner
        .interactions
        .subscription_progress(&owner.project, &member.envelope.id, member.activated_at())
        .await
        .unwrap()
        .unwrap();
    let normalized_props: SubscriptionDeliveryProgressProps = normalized.clone().into();
    assert!(normalized.roots().iter().all(|root| root.window.is_none()));
    let mut read = store.snapshot().await.unwrap();
    assert!(
        read.cursor_validities(&normalized_props.cursors())
            .await
            .unwrap()
            .values()
            .all(|fact| *fact == CursorValidity::Valid),
        "saved progress mixed a discarded suffix with current-line cursors"
    );
    read.finish().await.unwrap();
    assert_eq!(
        windows
            .rescan_missing_subscription_windows(&owner.reader, owner.clock.now())
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        windows
            .rescan_missing_subscription_windows(&owner.reader, owner.clock.now())
            .await
            .unwrap(),
        0
    );
    let (batch, settlement) = windows
        .select_subscription_notice(
            &owner.reader,
            std::slice::from_ref(&owner.root),
            owner.clock.now(),
            usize::MAX,
        )
        .await
        .unwrap();
    assert_eq!(batch.roots.len(), 1);
    assert_eq!(batch.roots[0].from, copied.retained);
    assert_eq!(batch.roots[0].through, current);
    assert_eq!(batch.roots[0].post_count, 2);
    windows
        .settle_subscription_batch(
            &owner.reader,
            &settlement,
            SubscriptionDeliveryOutcome::Accepted,
        )
        .await
        .expect("settlement must preserve the selected cursor's actual assigning epoch");
    assert_eq!(
        windows
            .rescan_missing_subscription_windows(&owner.reader, owner.clock.now())
            .await
            .unwrap(),
        0
    );
    let saved = owner
        .interactions
        .subscription_progress(&owner.project, &member.envelope.id, member.activated_at())
        .await
        .unwrap()
        .unwrap();
    let saved_root = saved
        .roots()
        .iter()
        .find(|root| root.thread_id == owner.root)
        .unwrap();
    assert_eq!(saved_root.delivered_through, current);
    assert!(saved_root.window.is_none());
    drop(windows);
    drop(store);
    drop(owner.into_directory());
}

#[tokio::test]
async fn unknown_epoch_and_future_position_cursors_are_refused_without_rewriting_progress() {
    let owner = OwnerFixture::new().await;
    let post = owner
        .post(&owner.root, "real post before invalid cursor")
        .await;
    owner
        .store
        .lock()
        .await
        .rescan_missing_subscription_windows(&owner.reader, owner.clock.now())
        .await
        .unwrap();
    let member = current_membership(&owner).await;
    let saved = owner
        .interactions
        .subscription_progress(&owner.project, &member.envelope.id, member.activated_at())
        .await
        .unwrap()
        .unwrap();
    for scan in [
        FeedCursor {
            epoch: EpochNumber::try_from(99).unwrap(),
            position: post.position,
        },
        FeedCursor {
            epoch: post.epoch,
            position: FeedPosition::try_from(post.position.get() + 100).unwrap(),
        },
    ] {
        let mut props: SubscriptionDeliveryProgressProps = saved.clone().into();
        props.scan_through = scan;
        let invalid = SubscriptionDeliveryProgress::new(props).unwrap();
        owner
            .interactions
            .put_subscription_progress(&invalid)
            .await
            .unwrap();
        let result = owner
            .store
            .lock()
            .await
            .rescan_missing_subscription_windows(&owner.reader, owner.clock.now())
            .await;
        let error = result.expect_err("invalid project cursor was accepted");
        assert_eq!(error.kind, message_board::BoardFailureKind::InvalidRecord);
        assert_eq!(error.stage, message_board::BoardFailureStage::Storage);
        let stored = owner
            .interactions
            .subscription_progress(&owner.project, &member.envelope.id, member.activated_at())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored, invalid, "refusal rewrote invalid local progress");
    }
    drop(owner.into_directory());
}
