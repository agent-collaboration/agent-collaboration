//! Real project/SDK completion and retirement; native clients and explicit ACK marks are fixtures.
use super::native_follow_mail_proofs::{follow_application_with_replication, frozen_follow};
use super::*;
use message_board::{EpochNumber, FeedCursor, FeedPosition, HandoffSeq, ProjectId};
use message_board_storage::MailHandoffItem;
use std::sync::atomic::{AtomicU64, Ordering};
#[path = "native_activation_frontier.rs"]
mod activation_history;

struct FrontierMarks(AtomicU64);
impl collaboration_contracts::ProjectReplicationPosition for FrontierMarks {
    fn acknowledged_mail_changes<'a>(
        &'a self,
        _: &'a ProjectId,
    ) -> collaboration_contracts::AcknowledgedMailChangesFuture<'a> {
        Box::pin(async {
            Ok(message_board_storage::AcknowledgedProjectChanges {
                settled_through: Some(self.0.load(Ordering::SeqCst)),
                ..Default::default()
            })
        })
    }
    fn replicated_through<'a>(
        &'a self,
        _: &'a ProjectId,
        epoch: EpochNumber,
    ) -> collaboration_contracts::ReplicatedPositionFuture<'a> {
        Box::pin(async move {
            Ok(FeedCursor {
                epoch,
                position: FeedPosition::ZERO,
            })
        })
    }
}

pub(super) async fn frontier(fixture: &OwnerFixture) -> HandoffSeq {
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let through = read.mail_settled_through().await.unwrap();
    read.finish().await.unwrap();
    through
}

#[tokio::test]
async fn a_terminal_follow_stays_above_frontier_until_its_generation_child_is_frozen_and_settled() {
    Box::pin(async {
        let fixture = Box::pin(OwnerFixture::new()).await;
        let runtime = fixture.runtime().await;
        let (app, native, publisher) =
            super::native_follow_mail_proofs::follow_application(&fixture, &runtime).await;
        let position = fixture.post(&fixture.root, "terminal original with a missing child").await;
        let request = runtime.requests.lock().await.recv().await.unwrap();
        let frozen = frozen_follow(&fixture).await;
        assert_eq!(frozen.len(), 1);
        let original = serde_json::to_value(&frozen[0]).unwrap();
        let item = frozen[0].id();
        assert_eq!(request.payload.push_id.as_str(), item.as_uuid().to_string());
        let store = fixture.registry.existing_project(&fixture.project).await.unwrap().unwrap();
        let reference: message_board::MailItemRef = item.as_uuid().to_string().try_into().unwrap();
        let mut read = store.snapshot().await.unwrap();
        let handoff = read.mail_handoff(&reference).await.unwrap().unwrap();
        let original_sequence = handoff.sequence;
        let MailHandoffItem::Delivery(envelope) = handoff.item else { panic!("original delivery"); };
        read.finish().await.unwrap();

        // Complete the real native call while the project lease cannot advance
        // its frontier. The original must still be above it when the group arrives.
        let expired = crate::test_project_runtime::expire_lease(&fixture.registry, &fixture.project, fixture.clock.as_ref()).await.unwrap();
        runtime.completions.send(DeliveryOutcome::Started).unwrap();
        fixture.await_push_state(&request.payload.push_id, PushDeliveryState::Delivered).await;
        assert_eq!(native.runtime().local_outcome_for_test(item).await.unwrap(),
            Some(fleet_sync_protocol::RecipientOutcome::Delivered {
                acceptance: collaboration_mail_protocol::CarrierAcceptance::Started,
            }));
        publisher.shutdown().await;
        assert!(frontier(&fixture).await < original_sequence);
        crate::test_project_runtime::renew_lease(&fixture.registry, &fixture.project, expired, fixture.clock.as_ref()).await.unwrap();
        fixture.policy(&fixture.root, message_board::SubscriptionMode::Deliver,
            message_board::WhenIdle::Hold, 1, 2).await;

        let mut read = store.snapshot().await.unwrap();
        let head = read.mail_handoff_head().await.unwrap();
        let group = read.mail_handoff_page(original_sequence, 100).await.unwrap().into_iter()
            .find_map(|row| match row.item {
                MailHandoffItem::CancelGroup(group)
                    if group.cause == message_board_storage::MailCancelGroupCause::GenerationChanged => Some(group),
                _ => None,
            }).expect("the real policy transition creates a later generation group");
        assert!(read.mail_cancel_expansion_for_target(&group.id, &reference).await.unwrap().is_none());
        read.finish().await.unwrap();
        let recovery = Arc::new(crate::NotificationPublisher::new(
            fixture.admission.clone(), fixture.interactions.clone(), Arc::downgrade(&native.runtime_handle()),
        ));

        // This is the actual frontier owner, without publication filling the gap.
        recovery.advance_project_frontier_for_test(&fixture.project).await.unwrap();
        let missing_child_prefix = frontier(&fixture).await;
        assert!(missing_child_prefix < original_sequence,
            "missing child let terminal original {original_sequence:?} cross frontier {missing_child_prefix:?}");
        let mut read = store.snapshot().await.unwrap();
        assert!(read.mail_cancel_expansion_for_target(&group.id, &reference).await.unwrap().is_none(),
            "the missing-child observation must not silently publish its own child");
        read.finish().await.unwrap();

        recovery.queue_project(fixture.project.clone()).await.unwrap();
        recovery.wait_project_idle_for_test(&fixture.project).await;
        let mut read = store.snapshot().await.unwrap();
        let child = read.mail_cancel_expansion_for_target(&group.id, &reference).await.unwrap()
            .expect("normal publication freezes the real association");
        read.finish().await.unwrap();
        assert_eq!(native.runtime().replay_frozen_cancel_and_observe(
            &envelope, &child.expansion.envelope,
            crate::mail_delivery_runtime::RemoteCancelReplay::QueryOnly,
        ).await.unwrap(), crate::mail_delivery_runtime::FrozenCancelSettlement::SettledWithoutPrevention);
        (async {
            while frontier(&fixture).await != head { tokio::task::yield_now().await; }
        }).await;
        assert_eq!(serde_json::to_value(frozen_follow(&fixture).await.first().unwrap()).unwrap(), original);
        assert_eq!(frozen_follow(&fixture).await.len(), 1);
        let record = fixture.interactions.get_push_record(&request.payload.push_id).await.unwrap().unwrap();
        assert_eq!(record.activity.unwrap().ranges, vec![collaboration_protocol::PushActivityRange {
            project_id: fixture.project.clone(), thread_id: fixture.root.clone(), from: position, through: position,
        }]);
        assert_eq!(runtime.completed_native_calls.load(Ordering::SeqCst), 1);
        assert!(runtime.requests.lock().await.try_recv().is_err());
        recovery.shutdown().await;
        drop(recovery);
        drop(publisher);
        drop(app);
        runtime.close().await;
        fixture.close_native(native).await;
    }).await;
}

#[tokio::test]
async fn two_prevented_generations_keep_a_resolved_drain_above_frontier_until_final_delivery() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) =
        super::native_follow_mail_proofs::follow_application(&fixture, &runtime).await;
    let position = fixture
        .post(
            &fixture.root,
            "one range survives two generation preventions and resolve",
        )
        .await;
    fixture.held_native_pushes(1).await;
    let initial = frozen_follow(&fixture).await;
    assert_eq!(initial.len(), 1);
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let id = initial[0].id().as_uuid().to_string().try_into().unwrap();
    let original_sequence = read.mail_handoff(&id).await.unwrap().unwrap().sequence;
    read.finish().await.unwrap();
    let mut before = Vec::new();
    for (quiet, cap, count) in [(1, 2, 2_usize), (2, 3, 3_usize)] {
        fixture
            .policy(
                &fixture.root,
                message_board::SubscriptionMode::Deliver,
                message_board::WhenIdle::Hold,
                quiet,
                cap,
            )
            .await;
        publisher
            .queue_project(fixture.project.clone())
            .await
            .unwrap();
        fixture
            .clock
            .advance(std::time::Duration::from_secs(cap))
            .unwrap();
        (async {
            loop {
                if frozen_follow(&fixture).await.len() == count {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;

        fixture.held_native_pushes(1).await;
        publisher.wait_project_idle_for_test(&fixture.project).await;
        before.push(frontier(&fixture).await);
    }
    fixture
        .resolve(
            message_board::ThreadResolveRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: message_board::ProtocolVersion::CURRENT,
                reason: "resolve still owes the prevented range"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                thread: fixture.root.clone().into(),
                actor: human("board-owner"),
                acting_for: None,
            },
            fixture.clock.utc_now(),
        )
        .await
        .unwrap();
    publisher
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    publisher.wait_project_idle_for_test(&fixture.project).await;
    before.push(frontier(&fixture).await);
    let items = frozen_follow(&fixture).await;
    let mut read = store.snapshot().await.unwrap();
    let head = read.mail_handoff_head().await.unwrap();
    read.finish().await.unwrap();
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    // Held mail waits for a signal that its target loaded, not for the clock.
    fixture.held_mail.observation_retried();
    let request = (async { runtime.requests.lock().await.recv().await.unwrap() }).await;
    let received = request.payload.push_id.clone();
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    (async {
        while frontier(&fixture).await != head {
            tokio::task::yield_now().await;
        }
    })
    .await;
    let mut outcomes = Vec::new();
    for item in &items {
        outcomes.push(
            native
                .runtime()
                .local_outcome_for_test(item.id())
                .await
                .unwrap(),
        );
    }
    let extra = runtime.requests.lock().await.try_recv().is_ok();
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    assert_eq!(items.len(), 3);
    assert!(
        before.iter().all(|prefix| *prefix < original_sequence),
        "a prevented replacement chain or resolved drain was prematurely settled: {before:?}"
    );

    assert!(matches!(
        outcomes.as_slice(),
        [
            Some(fleet_sync_protocol::RecipientOutcome::Prevented { .. }),
            Some(fleet_sync_protocol::RecipientOutcome::Prevented { .. }),
            Some(fleet_sync_protocol::RecipientOutcome::Delivered { .. })
        ]
    ));
    assert_eq!(received.as_str(), items[2].id().as_uuid().to_string());
    assert!(!extra);
    let mut previous = None;
    for item in items {
        let board_sync_protocol::BoardDeliveryBody::Notification(body) = item.body() else {
            panic!("follow");
        };
        let collaboration_mail_protocol::NotificationSource::FollowBatch {
            generation, roots, ..
        } = &body.source
        else {
            panic!("follow");
        };
        assert!(previous.is_none_or(|old| old < generation.get()));
        previous = Some(generation.get());
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].from(), position);
        assert_eq!(roots[0].through(), position);
        assert_eq!(roots[0].post_count(), 1);
    }
}

#[tokio::test]
async fn quiet_local_delivery_releases_acknowledged_frontier_without_erasing_retained_custody() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let marks = Arc::new(FrontierMarks(AtomicU64::new(0)));
    let (app, native, publisher) =
        follow_application_with_replication(&fixture, &runtime, None, marks.clone()).await;
    fixture
        .post(
            &fixture.root,
            "quiet completion must advance without another board hint",
        )
        .await;
    fixture.held_native_pushes(1).await;
    runtime.owner_barrier(&fixture.reader).await;
    publisher.wait_project_idle_for_test(&fixture.project).await;
    let frozen = frozen_follow(&fixture).await;
    assert_eq!(frozen.len(), 1);
    let initial_frontier = frontier(&fixture).await;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let feed_before = read.high_water();
    let head = read.mail_handoff_head().await.unwrap();
    let reference = frozen[0].id().as_uuid().to_string().try_into().unwrap();
    let frozen_sequence = read
        .mail_handoff(&reference)
        .await
        .unwrap()
        .unwrap()
        .sequence;
    read.finish().await.unwrap();
    eprintln!(
        "QUIET_FRONTIER_INITIAL {initial_frontier:?} FOLLOW_SEQUENCE {frozen_sequence:?} HEAD {head:?} SDK {:?}",
        native
            .runtime()
            .local_outcome_for_test(frozen[0].id())
            .await
            .unwrap()
    );

    // No board mutation, ACK, Welcome or explicit publisher prompt follows this completion.
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    // Held mail waits for a signal that its target loaded, not for the clock.
    fixture.held_mail.observation_retried();
    let request = (async { runtime.requests.lock().await.recv().await.unwrap() }).await;
    let push = request.payload.push_id.clone();
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    (async {
        loop {
            let through = frontier(&fixture).await;
            if through == head {
                break through;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let show = PushRecordShowParams {
        caller: target_session(&fixture.reader).unwrap(),
        reference: push.as_str().to_owned(),
    };
    let before_retirement = app.messages().push_show(show.clone()).await.unwrap();
    let retained_before_ack = native
        .runtime()
        .local_outcome_for_test(frozen[0].id())
        .await
        .unwrap();
    let progress_before_ack = fixture
        .interactions
        .mail_retired_through(&fixture.project)
        .await
        .unwrap();

    // The mark and this prompt stand in for the existing ACK port/handler.
    // This tests its release gate, not real replication or central acceptance.
    marks.0.store(head.get(), Ordering::SeqCst);
    publisher
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    (async {
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
    .await;

    let retained_after_ack = native
        .runtime()
        .local_outcome_for_test(frozen[0].id())
        .await
        .unwrap();
    let after_retirement = app.messages().push_show(show).await.unwrap();

    publisher
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    publisher.wait_project_idle_for_test(&fixture.project).await;

    let mut read = store.snapshot().await.unwrap();
    let feed_after = read.high_water();
    let handoffs_after = read.mail_handoff_head().await.unwrap();
    read.finish().await.unwrap();
    let no_repeat = runtime.requests.lock().await.try_recv().is_err();
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;

    assert!(
        initial_frontier < frozen_sequence,
        "the initial frontier passed the held follow"
    );
    assert!(matches!(
        retained_before_ack,
        Some(fleet_sync_protocol::RecipientOutcome::Delivered { .. })
    ));
    assert_eq!(progress_before_ack, HandoffSeq::ZERO);
    assert!(
        matches!(
            retained_after_ack,
            Some(fleet_sync_protocol::RecipientOutcome::Delivered { .. })
        ),
        "acknowledged frontier must release progress while retaining Delivered custody"
    );
    assert_eq!(
        serde_json::to_value(before_retirement).unwrap(),
        serde_json::to_value(after_retirement).unwrap()
    );
    assert_eq!(feed_before, feed_after);
    assert_eq!(head, handoffs_after);
    assert!(
        no_repeat,
        "a publisher replay re-admitted a retired handoff"
    );
}
