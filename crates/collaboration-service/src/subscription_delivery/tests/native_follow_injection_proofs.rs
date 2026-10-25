//! The local injection check persists a real control before allowing any native client call.
use super::native_follow_mail_proofs::{follow_application, frozen_follow};
use super::*;
use message_board::{CancelExpansionSeq, SubscriptionScope, ThreadSubscriptionUnsubscribeRequest};

#[tokio::test]
async fn a_written_unacknowledged_group_prevents_local_injection_without_calling_the_native_client()
{
    assert_local_injection_prevention(false).await;
}

#[tokio::test]
async fn a_native_forced_tail_keeps_prevention_after_the_group_is_discarded_from_the_successor_file()
 {
    assert_local_injection_prevention(true).await;
}

async fn assert_local_injection_prevention(discard_group: bool) {
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    fixture
        .post(
            &fixture.root,
            "written local cancellation wins before injection",
        )
        .await;
    let target = target_session(&fixture.reader).unwrap();
    let held = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if let Some(record) = native
                .interactions
                .list_held_mail_push_records(&target)
                .await
                .unwrap()
                .into_iter()
                .find(|record| record.kind == PushKind::SubscriptionActivity)
            {
                break record;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let kept_prefix = if discard_group {
        Some(
            super::native_cancel_takeover_fixture::KeptProjectPrefix::capture(&fixture)
                .await
                .unwrap(),
        )
    } else {
        None
    };
    let frozen = frozen_follow(&fixture).await;
    assert_eq!(frozen.len(), 1);
    // The first publisher pass must really be over before the group is written;
    // otherwise its proactive expansion could conceal a missing injection check.
    tokio::time::timeout(
        Duration::from_secs(6),
        publisher.wait_project_idle_for_test(&fixture.project),
    )
    .await
    .unwrap();
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
    // This installation has no central session or replication port. A local
    // control must not wait for a watermark it can never receive (U63 / 840).
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.clock.advance_without_tokio_time(30);
    let next_client_call = async { runtime.requests.lock().await.recv().await.unwrap() };
    let terminal = async {
        loop {
            let record = native
                .interactions
                .get_push_record(&held.push_id)
                .await
                .unwrap()
                .unwrap();
            if record.settled_at.is_some() {
                break record;
            }
            tokio::task::yield_now().await;
        }
    };
    let mut native_called = false;
    let terminal = tokio::time::timeout(Duration::from_secs(6), async {
        tokio::select! {
            record = terminal => record,
            request = next_client_call => {
                assert_eq!(request.payload.push_id, held.push_id);
                native_called = true;
                runtime.completions.send(DeliveryOutcome::Started).unwrap();
                await_native_completion(&runtime, 1).await;
                loop {
                    let record = native.interactions.get_push_record(&held.push_id).await.unwrap().unwrap();
                    if record.settled_at.is_some() { break record; }
                    tokio::task::yield_now().await;
                }
            }
        }
    }).await;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let expansions = read
        .mail_cancel_expansion_page(CancelExpansionSeq::ZERO, 100)
        .await
        .unwrap();
    read.finish().await.unwrap();
    let original_outcome = native
        .runtime()
        .local_outcome_for_test(frozen[0].id())
        .await
        .unwrap();
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
    let mut takeover_observed = false;
    if let Some(prefix) = kept_prefix
        && matches!(
            original_outcome,
            Some(fleet_sync_protocol::RecipientOutcome::Prevented { .. })
        )
    {
        let [child] = expansions.as_slice() else {
            panic!("one local control before takeover");
        };
        publisher.shutdown().await;
        runtime.service.shutdown().await;
        prefix
            .discard_later_group(
                &fixture,
                &child.expansion.group_handoff,
                &child.expansion.target_item,
            )
            .await
            .unwrap();
        let item = serde_json::from_value(serde_json::to_value(&frozen[0]).unwrap()).unwrap();
        tokio::time::timeout(
            Duration::from_secs(6),
            native.runtime().enqueue_local_project_item(item),
        )
        .await
        .unwrap()
        .unwrap();
        let after = native
            .runtime()
            .local_outcome_for_test(frozen[0].id())
            .await
            .unwrap();
        assert!(
            matches!(after, Some(fleet_sync_protocol::RecipientOutcome::Prevented { cancel }) if cancel == child.expansion.envelope.id()),
            "discarding the project group undid the SDK's decided prevention"
        );
        assert!(
            runtime.requests.lock().await.try_recv().is_err(),
            "takeover replay called the native client"
        );
        assert_eq!(
            runtime
                .completed_native_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        takeover_observed = true;
    }
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
    assert!(
        !native_called,
        "a written local CancelGroup was ignored at the actual native injection boundary"
    );
    assert_eq!(
        takeover_observed, discard_group,
        "the native tail rejection and successor file were not exercised"
    );
    let terminal = terminal.expect("written local cancel never settled");
    assert_eq!(terminal.delivery_state, PushDeliveryState::Rejected);
    assert!(
        matches!(terminal.last_outcome.as_ref().map(|outcome| &outcome.outcome), Some(DeliveryOutcome::NotSubmitted { reason, .. }) if reason == "mail cancelled before local delivery"),
        "{terminal:?}"
    );
    let [child] = expansions.as_slice() else {
        panic!("one persisted frozen local child");
    };
    assert_eq!(child.expansion.target_item.as_str(), held.push_id.as_str());
    assert!(
        matches!(original_outcome, Some(fleet_sync_protocol::RecipientOutcome::Prevented { cancel }) if cancel == child.expansion.envelope.id()),
        "the SDK did not decide Prevented for the original"
    );
    assert!(
        matches!(child_outcomes.as_slice(), [Some(fleet_sync_protocol::RecipientOutcome::Prevented { cancel })] if *cancel == child.expansion.envelope.id()),
        "the real child control did not settle Prevented"
    );
}

#[tokio::test]
async fn a_follow_recipient_with_no_sender_project_file_can_still_inject_normally() {
    use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
    use collaboration_mail_protocol::{
        MailSender, NotificationBody, NotificationSource, RecordPointer, RootRange,
    };
    use fleet_sync_protocol::{DeliveryItem, ItemId, Recipient};
    use message_board::{
        EpochNumber, FeedCursor, FeedPosition, Identity, ProjectId, RecordId,
        SubscriptionGeneration, SubscriptionPolicy,
    };
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    let project = ProjectId::generate();
    assert!(
        fixture
            .registry
            .existing_project(&project)
            .await
            .unwrap()
            .is_none()
    );
    let target = target_session(&fixture.reader).unwrap();
    let address: message_board::SessionRef = target.clone().try_into().unwrap();
    let position = FeedCursor {
        epoch: EpochNumber::INITIAL,
        position: FeedPosition::try_from(1).unwrap(),
    };
    let item = DeliveryItem::<BoardDeliveryTypes>::new(
        ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        MailSender::Project {
            project: project.clone(),
        },
        Recipient {
            peer: PeerId::try_from("0192a1b2-c3d4-7000-8000-000000000001".to_owned()).unwrap(),
            address: address.clone(),
        },
        None,
        BoardDeliveryBody::Notification(NotificationBody {
            excerpt: "a recipient does not need the sender's project file"
                .to_owned()
                .try_into()
                .unwrap(),
            pointer: RecordPointer {
                project,
                subject: fixture.root.clone().into(),
                position,
            },
            source: NotificationSource::FollowBatch {
                membership: RecordId::generate().as_str().to_owned().try_into().unwrap(),
                activated_at: FeedCursor {
                    position: FeedPosition::ZERO,
                    ..position
                },
                generation: SubscriptionGeneration::new(1).unwrap(),
                roots: vec![RootRange::new(fixture.root.clone(), position, position, 1).unwrap()],
                policy: SubscriptionPolicy::defaults_for(&Identity::Session { session: address }),
                draining: false,
            },
        }),
    )
    .unwrap();
    let reference = DeliveryItemRef {
        item: item.id(),
        digest: item.digest().clone(),
    };
    // Exercise native custody and the injection owner. The sender's transport
    // and replication policy are outside this recipient component case.
    let item = serde_json::from_value(serde_json::to_value(item).unwrap()).unwrap();
    // The owning API waits for settlement, including this client's answer.
    let mail = native.runtime_handle();
    let admission = tokio::spawn(async move { mail.enqueue_local_project_item(item).await });
    let received = tokio::time::timeout(Duration::from_secs(6), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await;
    if received.is_ok() {
        runtime.completions.send(DeliveryOutcome::Started).unwrap();
        await_native_completion(&runtime, 1).await;
        await_compaction(&native, reference.clone()).await;
    }
    tokio::time::timeout(Duration::from_secs(6), admission)
        .await
        .expect("native settlement hung")
        .unwrap()
        .unwrap();
    let outcome = tokio::time::timeout(
        Duration::from_secs(6),
        native.runtime().local_outcome_for_test(reference.item),
    )
    .await
    .expect("outcome read hung")
    .unwrap();
    tokio::time::timeout(Duration::from_secs(6), publisher.shutdown())
        .await
        .expect("publisher drain hung");
    drop(publisher);
    drop(app);
    tokio::time::timeout(Duration::from_secs(6), runtime.close())
        .await
        .expect("reader drain hung");
    tokio::time::timeout(Duration::from_secs(12), fixture.close_native(native))
        .await
        .expect("native drain hung");
    let received =
        received.expect("requiring a sender project file held a legitimate recipient indefinitely");
    assert_eq!(
        received.payload.push_id.as_str(),
        reference.item.as_uuid().to_string()
    );
    assert!(matches!(
        outcome,
        Some(fleet_sync_protocol::RecipientOutcome::Delivered { .. })
    ));
}
