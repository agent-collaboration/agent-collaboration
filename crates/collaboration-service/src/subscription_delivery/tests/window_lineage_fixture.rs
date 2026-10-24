//! A verified native seed retains one post and discards a later selected window suffix.
use super::follow_admission_proofs::current_membership;
use super::native_cancel_takeover_fixture::KeptProjectPrefix;
use super::*;
use interaction_storage::SubscriptionDeliveryProgress;
use message_board::{FeedCursor, SubscriptionBatchSettlement};

pub(super) struct CopiedReader {
    pub owner: OwnerFixture,
    pub retained: FeedCursor,
    pub boundary: FeedCursor,
    pub discarded: FeedCursor,
    pub progress: SubscriptionDeliveryProgress,
    pub selection: SubscriptionBatchSettlement,
}

pub(super) async fn copied_reader() -> CopiedReader {
    let source = OwnerFixture::new().await;
    let retained = source.post(&source.root, "retained followed post").await;
    source
        .store
        .lock()
        .await
        .rescan_missing_subscription_windows(&source.reader, source.clock.now())
        .await
        .unwrap();
    let source_store = source
        .registry
        .existing_project(&source.project)
        .await
        .unwrap()
        .unwrap();
    let read = source_store.snapshot().await.unwrap();
    let boundary = read.high_water();
    read.finish().await.unwrap();
    drop(source_store);
    let kept = KeptProjectPrefix::capture(&source).await.unwrap();
    let discarded = source.post(&source.root, "discarded followed post").await;
    source
        .store
        .lock()
        .await
        .rescan_missing_subscription_windows(&source.reader, source.clock.now())
        .await
        .unwrap();
    let (_, selection) = source
        .store
        .lock()
        .await
        .select_subscription_notice(
            &source.reader,
            std::slice::from_ref(&source.root),
            source.clock.now(),
            usize::MAX,
        )
        .await
        .unwrap();
    assert_eq!(selection.roots.len(), 1);
    let member = current_membership(&source).await;
    let progress = source
        .interactions
        .subscription_progress(&source.project, &member.envelope.id, member.activated_at())
        .await
        .unwrap()
        .unwrap();
    let project = source.project.clone();
    let root = source.root.clone();
    let discussion_subject = source.discussion_subject.clone();
    let source_directory = source.into_directory();
    let mut owner = OwnerFixture::new().await;
    kept.publish_successor(&owner, &project).await.unwrap();
    owner.project = project;
    owner.root = root;
    owner.discussion_subject = discussion_subject;
    drop(source_directory);
    CopiedReader {
        owner,
        retained,
        boundary,
        discarded,
        progress,
        selection,
    }
}
