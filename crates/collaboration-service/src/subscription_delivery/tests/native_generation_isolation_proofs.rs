//! A different reader's remote Query cannot block replacement or later publication.
use super::follow_admission_proofs::proposal;
use super::native_follow_mail_proofs::{follow_application_with_replication, frozen_follow};
use super::*;
use crate::frozen_mail_query_fixture as query_fixture;
use fleet_sync_protocol::{
    CancelBody, CancelResult, DeliveryCancel, DeliveryEnvelope, DeliveryItemRef, DeliveryStatus,
    ItemId, ItemStatus, ItemStatusState, StatusVersion, TooLateAt,
};
use message_board::*;
use message_board_storage::{MailCancelExpansion, MailCancelGroupCause, MailHandoffItem};

#[derive(Default)]
struct IsolationAcknowledgements(
    std::sync::Mutex<message_board_storage::AcknowledgedProjectChanges>,
);
impl crate::ProjectReplicationPosition for IsolationAcknowledgements {
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

#[tokio::test]
async fn local_generation_replacement_reaches_native_delivery_while_another_readers_query_is_held()
{
    // Arrange local L with actual Held custody before adding R in the same project.
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let peer = PeerId::try_from("0192a1b2-c3d4-7000-8000-000000000001".to_owned()).unwrap();
    let mut query = query_fixture::FrozenMailQuery::open(fixture.directory.path(), peer).await;
    let marks = Arc::new(IsolationAcknowledgements::default());
    let (app, native, publisher) = follow_application_with_replication(
        &fixture,
        &runtime,
        Some(query.session.clone()),
        marks.clone(),
    )
    .await;
    fixture
        .post(&fixture.root, "local activity remains owed")
        .await;
    let local_target = target_session(&fixture.reader).unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        while native
            .interactions
            .list_held_mail_push_records(&local_target)
            .await
            .unwrap()
            .is_empty()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let local_original = frozen_follow(&fixture).await.pop().unwrap();
    let (remote_member, remote_target, remote_child) =
        arrange_remote_generation_cancel(&fixture).await;
    let remote_publisher = publisher.clone();
    let project = fixture.project.clone();
    let resolving = tokio::spawn(async move {
        // This is the same SDK resolver used by reader refresh, not a synthetic request.
        remote_publisher
            .generation_preventions(&[project], &[remote_member])
            .await
    });
    let queried = tokio::time::timeout(Duration::from_secs(6), query.entered.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        queried,
        vec![remote_child.item],
        "the held wire request must name R's frozen child"
    );

    // Act: L also changes generation and gets a real SDK Prevented outcome.
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
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if matches!(
                native
                    .runtime()
                    .local_outcome_for_test(local_original.id())
                    .await
                    .unwrap(),
                Some(fleet_sync_protocol::RecipientOutcome::Prevented { .. })
            ) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.clock.advance_without_tokio_time(32);
    let received = tokio::time::timeout(Duration::from_secs(6), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await;
    if received.is_ok() {
        runtime.completions.send(DeliveryOutcome::Started).unwrap();
        await_native_completion(&runtime, 1).await;
    }
    // A completed replacement must not strand the next local post behind settlement.
    let later_received = if received.is_ok() {
        fixture
            .post(&fixture.root, "later local activity remains independent")
            .await;
        fixture.clock.advance_without_tokio_time(2);
        runtime
            .service
            .reconcile_reader(fixture.reader.clone())
            .await
            .unwrap();
        let mut hints = tokio::task::JoinSet::new();
        for _ in 0..16 {
            let publisher = publisher.clone();
            let project = fixture.project.clone();
            hints.spawn(async move { publisher.queue_project(project).await.unwrap() });
        }
        while let Some(hint) = hints.join_next().await {
            hint.unwrap();
        }
        Some(
            tokio::time::timeout(Duration::from_secs(6), async {
                runtime.requests.lock().await.recv().await.unwrap()
            })
            .await,
        )
    } else {
        None
    };
    if later_received.as_ref().is_some_and(Result::is_ok) {
        runtime.completions.send(DeliveryOutcome::Started).unwrap();
        await_native_completion(&runtime, 2).await;
    }
    // Only replication acknowledgement stands in; it names the actual frozen keys.
    // No settled-through acknowledgement is supplied, so this cannot authorize retirement.
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let acknowledged_handoffs = read.mail_handoff_head().await.unwrap();
    let acknowledged_expansions = read
        .mail_cancel_expansion_page(CancelExpansionSeq::ZERO, 100)
        .await
        .unwrap()
        .last()
        .unwrap()
        .sequence;
    read.finish().await.unwrap();
    *marks.0.lock().unwrap() = message_board_storage::AcknowledgedProjectChanges {
        max_handoff_seq: Some(acknowledged_handoffs.get()),
        max_expansion_seq: Some(acknowledged_expansions.get()),
        ..Default::default()
    };
    // Release and drain the real Query even on the intended wrong-build failure.
    for (reference, status, cancel) in [
        (
            &remote_child,
            DeliveryStatus::Cancelled,
            Some(CancelResult::TooLate {
                at: TooLateAt::Delivered,
            }),
        ),
        (
            &remote_target,
            DeliveryStatus::Delivered {
                acceptance: collaboration_mail_protocol::CarrierAcceptance::Started,
            },
            None,
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
    query.release();
    tokio::time::timeout(Duration::from_secs(6), resolving)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let late_received = if later_received.as_ref().is_some_and(Result::is_err) {
        let late = tokio::time::timeout(Duration::from_secs(6), async {
            runtime.requests.lock().await.recv().await.unwrap()
        })
        .await;
        if late.is_ok() {
            runtime.completions.send(DeliveryOutcome::Started).unwrap();
            await_native_completion(&runtime, 2).await;
        }
        Some(late)
    } else {
        None
    };
    let local_items: Vec<_> = frozen_follow(&fixture)
        .await
        .into_iter()
        .filter(|item| {
            matches!(&fixture.reader, Identity::Session { session }
            if item.recipient().address == *session)
        })
        .collect();
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let head = read.mail_handoff_head().await.unwrap();
    read.finish().await.unwrap();
    let frontier_advanced = tokio::time::timeout(Duration::from_secs(6), async {
        while super::native_frontier_proofs::frontier(&fixture).await != head {
            tokio::task::yield_now().await;
        }
    })
    .await
    .is_ok();
    publisher.wait_project_idle_for_test(&fixture.project).await;
    let duplicate = runtime.requests.lock().await.try_recv().is_ok();

    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    query.close().await;

    // Assert the native boundary happened before the unrelated Query was released.
    let received = received.expect("R's remote Query blocked L's native generation replacement");
    let later_received = later_received
        .expect("the first local replacement must complete")
        .unwrap_or_else(|error| {
            panic!(
                "R's remote Query blocked later L publication: {error}; delivered after release: {}",
                late_received.as_ref().is_some_and(Result::is_ok)
            )
        });
    assert_eq!(
        local_items.len(),
        3,
        "L must replace its prevented range once and publish the later activity"
    );
    assert_eq!(
        received.payload.push_id.as_str(),
        local_items[1].id().as_uuid().to_string()
    );
    assert_eq!(
        later_received.payload.push_id.as_str(),
        local_items[2].id().as_uuid().to_string()
    );
    assert!(
        frontier_advanced,
        "settlement failed to catch up after R's release"
    );
    assert!(
        !duplicate,
        "concurrent hints duplicated a native publication"
    );
}

#[tokio::test]
async fn an_expired_lease_queries_an_eligible_remote_cancel_and_renewal_replays_it_once() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.runtime().await;
    let peer = PeerId::try_from("0192a1b2-c3d4-7000-8000-000000000001".to_owned()).unwrap();
    let query = query_fixture::FrozenMailQuery::open(fixture.directory.path(), peer).await;
    query.accept_submissions();
    query.release();
    let marks = Arc::new(IsolationAcknowledgements::default());
    let (app, native, publisher) = follow_application_with_replication(
        &fixture,
        &runtime,
        Some(query.session.clone()),
        marks.clone(),
    )
    .await;
    let (member, target, child) = arrange_remote_generation_cancel(&fixture).await;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let highest = read.mail_handoff_head().await.unwrap();
    let expansion = read
        .mail_cancel_expansion_page(CancelExpansionSeq::ZERO, 100)
        .await
        .unwrap()
        .last()
        .unwrap()
        .sequence;
    read.finish().await.unwrap();
    *marks.0.lock().unwrap() = message_board_storage::AcknowledgedProjectChanges {
        max_handoff_seq: Some(highest.get()),
        max_expansion_seq: Some(expansion.get()),
        ..Default::default()
    };
    let expired = crate::test_project_runtime::expire_lease(&fixture.registry, &fixture.project)
        .await
        .unwrap();
    let observed = tokio::time::timeout(
        Duration::from_secs(6),
        publisher.generation_preventions(
            std::slice::from_ref(&fixture.project),
            std::slice::from_ref(&member),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(observed.is_empty());
    let id: collaboration_protocol::MailItemId =
        child.item.as_uuid().to_string().try_into().unwrap();
    assert!(
        fixture
            .interactions
            .mail_submission_epoch(&id)
            .await
            .unwrap()
            .is_none(),
        "expired writer replayed a control into native custody"
    );
    assert!(
        query.submissions().is_empty(),
        "expired writer submitted its remote control"
    );
    crate::test_project_runtime::renew_lease(&fixture.registry, &fixture.project, expired)
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(6),
        publisher.generation_preventions(
            std::slice::from_ref(&fixture.project),
            std::slice::from_ref(&member),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        while query.submissions().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(query.submissions(), vec![child.clone()]);
    for reference in [&child, &target] {
        query.answer_for(
            reference.item,
            ItemStatus {
                item: reference.item,
                state: ItemStatusState::Known {
                    digest: reference.digest.clone(),
                    status: DeliveryStatus::Cancelled,
                    version: StatusVersion::new(2),
                    cancel: if reference.item == child.item {
                        Some(CancelResult::Prevented)
                    } else {
                        None
                    },
                },
            },
        );
    }
    let observed = tokio::time::timeout(
        Duration::from_secs(6),
        publisher.generation_preventions(
            std::slice::from_ref(&fixture.project),
            std::slice::from_ref(&member),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].target(), &target);
    assert_eq!(observed[0].cancel(), &child);
    assert_eq!(query.submissions(), vec![child]);
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    query.close().await;
}

pub(super) async fn arrange_remote_generation_cancel(
    fixture: &OwnerFixture,
) -> (SubscriptionMembership, DeliveryItemRef, DeliveryItemRef) {
    let root = ThreadId::generate();
    fixture
        .create_root(
            &root,
            human(&format!("remote-root-author-{}", root.as_str())),
        )
        .await;
    let remote_service: ServiceId = "00000000-0000-4000-8000-000000000099"
        .to_owned()
        .try_into()
        .unwrap();
    let reader = Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: remote_service.clone(),
                endpoint_id: "codex-local".to_owned().try_into().unwrap(),
            },
            session_id: "isolated-remote-reader".to_owned().try_into().unwrap(),
        },
    };
    fixture
        .join(
            &root,
            reader.clone(),
            ParticipantRole::Participant,
            true,
            None,
        )
        .await;
    let position = fixture.post(&root, "remote generation debt").await;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let member = read
        .membership(&reader, &SubscriptionScope::thread(root.clone()))
        .await
        .unwrap()
        .unwrap();
    read.finish().await.unwrap();
    let mut input = proposal(fixture, &member, position);
    input.roots = vec![
        collaboration_mail_protocol::RootRange::new(root.clone(), position, position, 1).unwrap(),
    ];
    input.enrollment = crate::IntentRecipientEnrollment {
        service: remote_service,
        peer: PeerId::new(uuid::Uuid::now_v7()).unwrap(),
    };
    let target = fixture
        .admission
        .allocate_follow_batch(&fixture.project, input, fixture.clock.now())
        .await
        .unwrap()
        .unwrap();
    fixture
        .subscribe(
            ThreadSubscriptionSubscribeRequest {
                reader,
                scope: SubscriptionScope::thread(root),
                policy: SubscriptionPolicyPatch {
                    mode: Some(SubscriptionMode::Deliver),
                    when_idle: Some(WhenIdle::Hold),
                    timing: SubscriptionTimingPatch {
                        quiet_seconds: Some(1),
                        cap_seconds: Some(2),
                    },
                    lifetime_seconds: None,
                },
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    let mut write = store
        .begin_admission(fixture.admission.record_origin())
        .await
        .unwrap();
    let group = write
        .mail_handoff_page(HandoffSeq::ZERO, 100)
        .await
        .unwrap()
        .into_iter()
        .find_map(|row| {
            if let MailHandoffItem::CancelGroup(group) = row.item
                && group.membership.as_str() == member.envelope.id.as_str()
                && group.cause == MailCancelGroupCause::GenerationChanged
            {
                Some(group)
            } else {
                None
            }
        })
        .unwrap();
    let MailHandoffItem::Delivery(envelope) =
        write.mail_handoff(&target).await.unwrap().unwrap().item
    else {
        panic!("remote target")
    };
    let child = DeliveryCancel::new(
        ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        envelope.sender().clone(),
        envelope.recipient().clone(),
        None,
        CancelBody {
            target: envelope.id(),
        },
    )
    .unwrap();
    let target_ref = DeliveryItemRef {
        item: envelope.id(),
        digest: envelope.digest().clone(),
    };
    let child_ref = DeliveryItemRef {
        item: child.id(),
        digest: child.digest().clone(),
    };
    write
        .append_mail_cancel_expansion(&MailCancelExpansion {
            group_handoff: group.id,
            target_item: target,
            envelope: Box::new(DeliveryEnvelope::Cancel(child)),
        })
        .await
        .unwrap();
    let current = write
        .membership(member.reader(), member.scope())
        .await
        .unwrap()
        .unwrap();
    write.commit().await.unwrap();
    (current, target_ref, child_ref)
}
