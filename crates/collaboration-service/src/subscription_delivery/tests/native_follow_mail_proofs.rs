//! Follow allocation uses the project transaction and native custody, including held work.
use super::*;
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::NotificationSource;
use message_board::HandoffSeq;
use message_board_storage::MailHandoffItem;

pub(in crate::service_identity::subscription_delivery) async fn follow_application(
    fixture: &OwnerFixture,
    runtime: &OwnerRuntime,
) -> (
    Arc<CollaborationApplication>,
    NativeMailOwner,
    Arc<crate::NotificationPublisher>,
) {
    follow_application_with_session(fixture, runtime, None).await
}

pub(super) async fn follow_application_with_session(
    fixture: &OwnerFixture,
    runtime: &OwnerRuntime,
    session: Option<crate::MailDeliverySession>,
) -> (
    Arc<CollaborationApplication>,
    NativeMailOwner,
    Arc<crate::NotificationPublisher>,
) {
    follow_application_with_replication(
        fixture,
        runtime,
        session,
        Arc::new(crate::UnacknowledgedProjectReplication),
    )
    .await
}

pub(super) async fn follow_application_with_replication(
    fixture: &OwnerFixture,
    runtime: &OwnerRuntime,
    session: Option<crate::MailDeliverySession>,
    replication: Arc<dyn crate::ProjectReplicationPosition>,
) -> (
    Arc<CollaborationApplication>,
    NativeMailOwner,
    Arc<crate::NotificationPublisher>,
) {
    let peer = PeerId::new(uuid::Uuid::parse_str("0192a1b2-c3d4-7000-8000-000000000001").unwrap())
        .unwrap();
    let interactions = fixture.interactions.clone();
    let (identity, native) = NativeMailOwner::start_with_session(
        ServiceIdentity::new(SERVICE_ID, SERVICE_ID)
            .unwrap()
            .with_project_replication(replication)
            .with_automation_store(fixture.automation.clone())
            .with_project_admission(fixture.admission.clone())
            .with_subscription_delivery_service(runtime.service.clone(), fixture.presence.clone()),
        peer,
        crate::MailPushDeliveryProps {
            interactions,
            actors: runtime.service.clone(),
            presence: fixture.presence.clone(),
            machine: crate::MachineIdentity::new(SERVICE_ID.to_owned().try_into().unwrap(), None)
                .unwrap(),
            clock: runtime.service.subscription_clock(),
        },
        session,
    )
    .unwrap();
    let publisher = Arc::new(crate::NotificationPublisher::new(
        fixture.admission.clone(),
        fixture.interactions.clone(),
        Arc::downgrade(&native.runtime_handle()),
    ));
    let app = Arc::new(CollaborationApplication::new(
        identity.with_notification_publisher(publisher.clone()),
    ));
    (app, native, publisher)
}

pub(in crate::service_identity::subscription_delivery) async fn frozen_follow(
    fixture: &OwnerFixture,
) -> Vec<fleet_sync_protocol::DeliveryItem<BoardDeliveryTypes>> {
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let handoffs = read.mail_handoff_page(HandoffSeq::ZERO, 100).await.unwrap();
    read.finish().await.unwrap();
    handoffs.into_iter().filter_map(|handoff| {
        let MailHandoffItem::Delivery(envelope) = handoff.item else { return None; };
        let fleet_sync_protocol::DeliveryEnvelope::Item(item) = *envelope else { return None; };
        matches!(item.body(), BoardDeliveryBody::Notification(body) if matches!(body.source, NotificationSource::FollowBatch { .. })).then_some(item)
    }).collect()
}

#[tokio::test]
async fn held_follow_keeps_its_frozen_item_ranges_and_link_until_one_native_acceptance() {
    // Arrange the real reader, project owner and native mail owner before the post.
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    let through = fixture
        .post(&fixture.root, "same-generation native follow")
        .await;
    let target = target_session(&fixture.reader).unwrap();
    let held = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let records = native
                .interactions
                .list_held_mail_push_records(&target)
                .await
                .unwrap();
            if let Some(record) = records
                .into_iter()
                .find(|record| record.kind == PushKind::SubscriptionActivity)
            {
                break record;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    // Clean up even when today's legacy C5 cannot allocate native follow mail.
    let held = match held {
        Ok(held) => held,
        Err(error) => {
            publisher.shutdown().await;
            drop(publisher);
            drop(app);
            runtime.close().await;
            fixture.close_native(native).await;
            panic!("native follow handoff missing: {error}");
        }
    };
    let frozen = frozen_follow(&fixture).await;
    if frozen.len() != 1 {
        let count = frozen.len();
        publisher.shutdown().await;
        drop(publisher);
        drop(app);
        runtime.close().await;
        fixture.close_native(native).await;
        assert_eq!(
            count, 1,
            "same-generation follow was allocated more than once"
        );
        return;
    }
    assert_eq!(frozen.len(), 1);
    let original = serde_json::to_value(&frozen[0]).unwrap();
    assert_eq!(held.push_id.as_str(), frozen[0].id().as_uuid().to_string());
    let BoardDeliveryBody::Notification(body) = frozen[0].body() else {
        panic!("follow body");
    };
    let NotificationSource::FollowBatch {
        roots, draining, ..
    } = &body.source
    else {
        panic!("follow source");
    };
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].root(), &fixture.root);
    assert_eq!(roots[0].from(), through);
    assert_eq!(roots[0].through(), through);
    assert_eq!(roots[0].post_count(), 1);
    assert!(!draining);
    let show = PushRecordShowParams {
        caller: target,
        reference: held.push_id.as_str().to_owned(),
    };
    let first = app.messages().push_show(show.clone()).await.unwrap();
    assert_eq!(first.record.delivery_state, PushDeliveryState::Held);
    assert_eq!(first.activity_ranges.len(), 1);
    assert_eq!(first.activity_ranges[0].posts.len(), 1);
    assert_eq!(
        first.activity_ranges[0].posts[0].text.as_str(),
        "same-generation native follow"
    );
    assert!(runtime.requests.lock().await.try_recv().is_err());

    // Act through the existing readiness cadence, without calling release_held.
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    fixture.clock.advance_without_tokio_time(30);
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
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

    // Assert the public link and frozen producer ranges survive repeated scans.
    let last = app.messages().push_show(show).await.unwrap();
    assert_eq!(last.link, first.link);
    assert_eq!(last.record.delivery_state, PushDeliveryState::Delivered);
    assert_eq!(
        serde_json::to_value(&last.activity_ranges).unwrap(),
        serde_json::to_value(&first.activity_ranges).unwrap()
    );
    runtime
        .service
        .reconcile_reader(fixture.reader.clone())
        .await
        .unwrap();
    runtime.owner_barrier(&fixture.reader).await;
    let final_items = frozen_follow(&fixture).await;
    assert_eq!(final_items.len(), 1);
    assert_eq!(serde_json::to_value(&final_items[0]).unwrap(), original);
    publisher.shutdown().await;
    drop(publisher);
    runtime.service.shutdown().await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    assert_eq!(
        runtime
            .completed_native_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}
