//! Verified seed and saved retirement floors exclude old custody without skipping new work.
use super::native_cancel_takeover_fixture::KeptProjectPrefix;
use super::native_follow_mail_proofs::follow_application_with_replication;
use super::native_generation_isolation_proofs::arrange_remote_generation_cancel;
use super::*;
use crate::frozen_mail_query_fixture::FrozenMailQuery;
use fleet_sync_protocol::{
    CancelResult, DeliveryItemRef, DeliveryStatus, ItemStatus, ItemStatusState, StatusVersion,
    TooLateAt,
};
use message_board::{
    CancelExpansionSeq, EpochNumber, FeedCursor, FeedPosition, HandoffSeq, ProjectId,
};
use message_board_storage::AcknowledgedProjectChanges;

struct FrontierAcknowledgements(std::sync::Mutex<AcknowledgedProjectChanges>);
impl crate::ProjectReplicationPosition for FrontierAcknowledgements {
    fn acknowledged_mail_changes<'a>(
        &'a self,
        _: &'a ProjectId,
    ) -> crate::AcknowledgedMailChangesFuture<'a> {
        Box::pin(async { Ok(self.0.lock().unwrap().clone()) })
    }
    fn replicated_through<'a>(
        &'a self,
        _: &'a ProjectId,
        epoch: EpochNumber,
    ) -> crate::ReplicatedPositionFuture<'a> {
        Box::pin(async move {
            Ok(FeedCursor {
                epoch,
                position: FeedPosition::ZERO,
            })
        })
    }
}
fn installation_peer() -> PeerId {
    PeerId::try_from("0192a1b2-c3d4-7000-8000-000000000001".to_owned()).unwrap()
}
fn answer_terminal_pair(
    query: &FrozenMailQuery,
    original: &DeliveryItemRef,
    child: &DeliveryItemRef,
) {
    for (reference, status, cancel) in [
        (
            original,
            DeliveryStatus::Delivered {
                acceptance: collaboration_mail_protocol::CarrierAcceptance::Started,
            },
            None,
        ),
        (
            child,
            DeliveryStatus::Cancelled,
            Some(CancelResult::TooLate {
                at: TooLateAt::Delivered,
            }),
        ),
    ] {
        query.answer_for(
            reference.item,
            ItemStatus {
                item: reference.item,
                state: ItemStatusState::Known {
                    digest: reference.digest.clone(),
                    status,
                    version: StatusVersion::new(2),
                    cancel,
                },
            },
        );
    }
}
async fn captured_marks(
    fixture: &OwnerFixture,
    settled: Option<HandoffSeq>,
) -> AcknowledgedProjectChanges {
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let marks = AcknowledgedProjectChanges {
        max_handoff_seq: Some(read.mail_handoff_head().await.unwrap().get()),
        max_expansion_seq: read
            .mail_cancel_expansion_page(CancelExpansionSeq::ZERO, 100)
            .await
            .unwrap()
            .last()
            .map(|row| row.sequence.get()),
        settled_through: settled.map(HandoffSeq::get),
        ..Default::default()
    };
    read.finish().await.unwrap();
    marks
}
struct PreparedSuccessor {
    owner: OwnerFixture,
    baseline: HandoffSeq,
    feed: FeedCursor,
    marks: AcknowledgedProjectChanges,
}

// This setup spans several physical owners. Keep its state on the heap so
// callers do not inline it beside the native migration/parser call stack.
fn seeded_successor() -> impl std::future::Future<Output = PreparedSuccessor> {
    Box::pin(async {
        // Only directory/grant acceptance, ACK keys and canonical remote answers stand in.
        let source = Box::pin(OwnerFixture::new()).await;
        let project = source.project.clone();
        let root = source.root.clone();
        let discussion_subject = source.discussion_subject.clone();
        let source_runtime = source.runtime().await;
        let (_, original, child) = arrange_remote_generation_cancel(&source).await;
        let source_query =
            FrozenMailQuery::open(source.directory.path(), installation_peer()).await;
        source_query.accept_submissions();
        source_query.release();
        answer_terminal_pair(&source_query, &original, &child);
        let acknowledgements = Arc::new(FrontierAcknowledgements(std::sync::Mutex::new(
            captured_marks(&source, None).await,
        )));
        let (app, native, publisher) = follow_application_with_replication(
            &source,
            &source_runtime,
            Some(source_query.session.clone()),
            acknowledgements.clone(),
        )
        .await;
        publisher.queue_project(project.clone()).await.unwrap();
        publisher.wait_project_idle_for_test(&project).await;
        let store = source
            .registry
            .existing_project(&project)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(store.verified_seed_frontier().await.unwrap(), None);
        let mut read = store.snapshot().await.unwrap();
        let baseline = read.mail_settled_through().await.unwrap();
        assert_eq!(baseline, read.mail_handoff_head().await.unwrap());
        assert!(baseline > HandoffSeq::ZERO);
        let feed = read.high_water();
        read.finish().await.unwrap();
        *acknowledgements.0.lock().unwrap() = captured_marks(&source, Some(baseline)).await;
        publisher.queue_project(project.clone()).await.unwrap();
        publisher.wait_project_idle_for_test(&project).await;
        assert_eq!(
            source
                .interactions
                .mail_retired_through(&project)
                .await
                .unwrap(),
            baseline
        );
        assert!(
            !source_query.retirement_requests().is_empty(),
            "e1 setup must retire through the real SDK/session route"
        );
        publisher.shutdown().await;
        drop(publisher);
        drop(app);
        drop(store);
        source_runtime.close().await;
        let source = source.reopen_native(native).await;
        source_query.close().await;

        // The e1 owner keeps its real saved progress across a physical native reopen.
        let runtime = source.runtime().await;
        let mut query = FrozenMailQuery::open(source.directory.path(), installation_peer()).await;
        query.release();
        let (app, native, publisher) = follow_application_with_replication(
            &source,
            &runtime,
            Some(query.session.clone()),
            acknowledgements,
        )
        .await;
        publisher.queue_project(project.clone()).await.unwrap();
        publisher.wait_project_idle_for_test(&project).await;
        let old_query = query.entered.try_recv().ok();
        let progress = source
            .interactions
            .mail_retired_through(&project)
            .await
            .unwrap();
        let kept = KeptProjectPrefix::capture(&source).await.unwrap();
        publisher.shutdown().await;
        drop(publisher);
        drop(app);
        runtime.close().await;
        source.close_native(native).await;
        query.close().await;
        assert!(
            old_query.is_none(),
            "e1 ignored its saved retirement progress: {old_query:?}"
        );
        assert_eq!(progress, baseline);

        // A distinct verified epoch file receives no old interaction/outbox/progress file.
        let mut successor = Box::pin(OwnerFixture::new()).await;
        let install = kept.publish_successor(&successor, &project).await.unwrap();
        successor.project = project.clone();
        successor.root = root;
        successor.discussion_subject = discussion_subject;
        let store = successor
            .registry
            .existing_project(&project)
            .await
            .unwrap()
            .unwrap();
        let receipt = successor
            .registry
            .database(&project)
            .await
            .unwrap()
            .installed_snapshot()
            .await
            .unwrap()
            .unwrap();
        assert!(receipt.verified);
        assert_eq!(receipt.install, install);
        let marks = store.acknowledged_capture(install.position).await.unwrap();
        assert_eq!(
            store.verified_seed_frontier().await.unwrap(),
            Some(baseline)
        );
        assert_eq!(marks.settled_through, Some(baseline.get()));
        drop(store);
        PreparedSuccessor {
            owner: successor,
            baseline,
            feed,
            marks,
        }
    })
}

#[tokio::test]
async fn a_successor_starts_retirement_at_the_seeded_frontier_without_replaying_old_custody() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let PreparedSuccessor {
            owner: successor,
            baseline,
            feed,
            marks,
        } = seeded_successor().await;
        let project = successor.project.clone();
        let store = successor
            .registry
            .existing_project(&project)
            .await
            .unwrap()
            .unwrap();
        let runtime = successor.runtime().await;
        let mut query =
            FrozenMailQuery::open(successor.directory.path(), installation_peer()).await;
        query.release();
        let (app, native, publisher) = follow_application_with_replication(
            &successor,
            &runtime,
            Some(query.session.clone()),
            Arc::new(FrontierAcknowledgements(std::sync::Mutex::new(marks))),
        )
        .await;
        publisher.queue_project(project.clone()).await.unwrap();
        publisher.wait_project_idle_for_test(&project).await;
        let progress = successor
            .interactions
            .mail_retired_through(&project)
            .await
            .unwrap();
        let old_query = query.entered.try_recv().ok();
        let submissions = query.submissions();
        let mut read = store.snapshot().await.unwrap();
        let final_frontier = read.mail_settled_through().await.unwrap();
        let final_head = read.mail_handoff_head().await.unwrap();
        let final_feed = read.high_water();
        read.finish().await.unwrap();
        publisher.shutdown().await;
        drop(publisher);
        drop(app);
        drop(store);
        runtime.close().await;
        successor.close_native(native).await;
        query.close().await;
        assert!(
            old_query.is_none(),
            "successor queried predecessor custody below the seeded frontier: {old_query:?}"
        );
        assert_eq!(
            progress,
            HandoffSeq::ZERO,
            "raising the seed floor must not fabricate saved retirement progress"
        );
        assert!(
            submissions.is_empty(),
            "successor submitted an already covered handoff"
        );
        assert_eq!(final_frontier, baseline);
        assert_eq!(final_head, baseline);
        assert_eq!(final_feed, feed);
    })
    .await
    .expect("bounded actual seeded successor retirement baseline");
}

#[tokio::test]
async fn a_live_frontier_above_the_seed_retires_new_items_and_a_reheld_owner_keeps_saved_progress()
{
    Box::pin(tokio::time::timeout(Duration::from_secs(45), async {
        let PreparedSuccessor {
            owner: successor,
            baseline,
            ..
        } = seeded_successor().await;
        let project = successor.project.clone();
        let (_, original, child) = arrange_remote_generation_cancel(&successor).await;
        let runtime = successor.runtime().await;
        let query = FrozenMailQuery::open(successor.directory.path(), installation_peer()).await;
        query.accept_submissions();
        query.release();
        answer_terminal_pair(&query, &original, &child);
        let acknowledgements = Arc::new(FrontierAcknowledgements(std::sync::Mutex::new(
            captured_marks(&successor, Some(baseline)).await,
        )));
        let (app, native, publisher) = follow_application_with_replication(
            &successor,
            &runtime,
            Some(query.session.clone()),
            acknowledgements.clone(),
        )
        .await;
        publisher.queue_project(project.clone()).await.unwrap();
        publisher.wait_project_idle_for_test(&project).await;
        let store = successor
            .registry
            .existing_project(&project)
            .await
            .unwrap()
            .unwrap();
        let mut read = store.snapshot().await.unwrap();
        let live = read.mail_settled_through().await.unwrap();
        assert_eq!(live, read.mail_handoff_head().await.unwrap());
        assert!(live > baseline);
        read.finish().await.unwrap();
        assert_eq!(
            store.verified_seed_frontier().await.unwrap(),
            Some(baseline),
            "live frontier contaminated the exact seed row"
        );
        *acknowledgements.0.lock().unwrap() = captured_marks(&successor, Some(live)).await;
        publisher.queue_project(project.clone()).await.unwrap();
        publisher.wait_project_idle_for_test(&project).await;
        let progress = successor
            .interactions
            .mail_retired_through(&project)
            .await
            .unwrap();
        let retired: std::collections::BTreeSet<_> = query
            .retirement_requests()
            .into_iter()
            .flat_map(|(_, items)| items)
            .collect();
        let expected = std::collections::BTreeSet::from([original.item, child.item]);
        publisher.shutdown().await;
        drop(publisher);
        drop(app);
        drop(store);
        runtime.close().await;
        let successor = successor.reopen_native(native).await;
        query.close().await;
        assert_eq!(
            retired, expected,
            "using the live frontier skipped actual post-seed retirements"
        );
        assert_eq!(
            progress, live,
            "actual post-seed retirement did not persist progress"
        );

        let expired = crate::test_project_runtime::expire_lease(&successor.registry, &project)
            .await
            .unwrap();
        crate::test_project_runtime::renew_lease(&successor.registry, &project, expired)
            .await
            .unwrap();
        let runtime = successor.runtime().await;
        let mut query =
            FrozenMailQuery::open(successor.directory.path(), installation_peer()).await;
        query.release(); // Re-held owner has no scripted knowledge of already retired items.
        let (app, native, publisher) = follow_application_with_replication(
            &successor,
            &runtime,
            Some(query.session.clone()),
            acknowledgements,
        )
        .await;
        publisher.queue_project(project.clone()).await.unwrap();
        publisher.wait_project_idle_for_test(&project).await;
        let old_query = query.entered.try_recv().ok();
        let reheld_progress = successor
            .interactions
            .mail_retired_through(&project)
            .await
            .unwrap();
        publisher.shutdown().await;
        drop(publisher);
        drop(app);
        runtime.close().await;
        successor.close_native(native).await;
        query.close().await;
        assert!(
            old_query.is_none(),
            "re-held owner ignored progress greater than the seed: {old_query:?}"
        );
        assert_eq!(reheld_progress, live);
    }))
    .await
    .expect("bounded post-seed retirement and re-held saved progress");
}

#[tokio::test]
async fn an_inherited_reader_scans_new_epoch_activity_from_its_valid_activation_cursor() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let PreparedSuccessor { owner, .. } = seeded_successor().await;
        let project = owner.project.clone();
        let posted = owner.post(&owner.root, "one new-epoch followed post").await;
        let store = owner.registry.existing_project(&project).await.unwrap().unwrap();
        let mut read = store.snapshot().await.unwrap();
        let member = read.membership(&owner.reader, &message_board::SubscriptionScope::thread(owner.root.clone())).await.unwrap().unwrap();
        let head = read.high_water();
        assert!(head.epoch > member.activated_at().epoch);
        assert!(matches!(read.cursor_validity(member.activated_at()).await.unwrap(), message_board_storage::CursorValidity::Valid));
        read.finish().await.unwrap();
        let rescanned = owner.store.lock().await.rescan_missing_subscription_windows(&owner.reader, owner.clock.now()).await;
        let progress = owner.interactions.subscription_progress(&project, &member.envelope.id, member.activated_at()).await.unwrap();
        drop(store);
        let root = owner.root.clone();
        drop(owner.into_directory());
        assert!(rescanned.is_ok(), "accepted inherited cursor was rejected for new-epoch activity: activation {:?}, head {:?}, result {rescanned:?}", member.activated_at(), head);
        assert_eq!(rescanned.unwrap(), 1);
        let progress = progress.expect("new-epoch scan must persist its actual window");
        assert_eq!(progress.scan_through(), head);
        let root = progress.roots().iter().find(|record| record.thread_id == root).unwrap();
        let window = root.window.as_ref().unwrap();
        assert_eq!(window.from, posted);
        assert_eq!(window.through, posted);
        assert_eq!(window.post_count, 1);
    }).await.expect("bounded inherited reader and new-epoch post");
}
