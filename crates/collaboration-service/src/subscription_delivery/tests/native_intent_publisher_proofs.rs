//! Project claims reach the native owner without a configured central session.
use super::*;
use message_board::{
    BoardOperation, HandoffSeq, IntentState, NotifyIntent, NotifyTarget, PostId, PostText, Record,
    RecordKind,
};
use message_board_storage::MailHandoffItem;

pub(super) async fn create_addressed_intent(fixture: &OwnerFixture) -> NotifyIntent {
    let post = PostId::generate();
    fixture
        .write_as(
            human("native-intent-writer"),
            BoardOperation::ThreadPost {
                post_id: post.clone(),
                thread_id: fixture.root.clone(),
                text: PostText::try_from("real project notification".to_owned()).unwrap(),
                references: Vec::new(),
                addressees: vec![NotifyTarget::Actor(fixture.reader.clone())],
            },
            fixture.clock.now(),
        )
        .await;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let intent = read
        .records(RecordKind::NotifyIntent)
        .await
        .unwrap()
        .into_iter()
        .find_map(|record| match record {
            Record::NotifyIntent(intent) if intent.cause == post.clone().into() => Some(intent),
            _ => None,
        })
        .unwrap();
    read.finish().await.unwrap();
    intent
}

fn installation_peer() -> PeerId {
    PeerId::new(uuid::Uuid::parse_str("0192a1b2-c3d4-7000-8000-000000000001").unwrap()).unwrap()
}

pub(super) async fn frozen_item(
    fixture: &OwnerFixture,
    intent: &NotifyIntent,
) -> fleet_sync_protocol::DeliveryItem<crate::RouterDeliveryTypes> {
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let Some(Record::NotifyIntent(current)) =
        read.record(&intent.intent_id.clone().into()).await.unwrap()
    else {
        panic!("intent");
    };
    let IntentState::HandedOff { item } = current.state() else {
        panic!("handed off");
    };
    let handoff = read.mail_handoff(item).await.unwrap().unwrap();
    assert_eq!(
        read.mail_handoff_page(HandoffSeq::ZERO, 10)
            .await
            .unwrap()
            .into_iter()
            .filter(|handoff| matches!(handoff.item, MailHandoffItem::Delivery(_)))
            .count(),
        1
    );
    read.finish().await.unwrap();
    let MailHandoffItem::Delivery(envelope) = handoff.item else {
        panic!("delivery");
    };
    let fleet_sync_protocol::DeliveryEnvelope::Item(frozen) = *envelope else {
        panic!("item");
    };
    assert_eq!(item.as_str(), frozen.id().as_uuid().to_string());
    serde_json::from_value(serde_json::to_value(frozen).unwrap()).unwrap()
}

#[tokio::test]
async fn a_frozen_local_intent_uses_native_custody_and_replay_does_not_recreate_its_pruned_push() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.runtime_without_board().await;
    let (app, native) = native_application(&fixture, &runtime, installation_peer());
    let publisher = crate::NotificationPublisher::new(
        fixture.admission.clone(),
        fixture.interactions.clone(),
        Arc::downgrade(&native.runtime_handle()),
    );
    let intent = create_addressed_intent(&fixture).await;
    publisher
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    let frozen = frozen_item(&fixture, &intent).await;
    let reference = DeliveryItemRef {
        item: frozen.id(),
        digest: frozen.digest().clone(),
    };
    assert_eq!(
        request.payload.push_id.as_str(),
        frozen.id().as_uuid().to_string()
    );
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    await_compaction(&native, reference.clone()).await;
    let shown = app
        .messages()
        .push_show(PushRecordShowParams {
            caller: target_session(&fixture.reader).unwrap(),
            reference: request.payload.push_id.as_str().to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(shown.record.delivery_state, PushDeliveryState::Delivered);
    assert!(shown.link.ends_with(request.payload.push_id.as_str()));
    assert!(
        matches!(shown.record.header_facts, PushHeaderFacts::BoardNotification { intent: ref delivered, .. } if delivered == &intent.intent_id)
    );
    fixture.assert_no_legacy_message_tables().await;
    assert_eq!(
        native
            .interactions
            .prune_push_records(fixture.clock.now() + chrono::Duration::days(40), 500)
            .await
            .unwrap(),
        1
    );
    native
        .runtime()
        .enqueue_local_project_item(frozen)
        .await
        .unwrap();
    assert!(
        native
            .interactions
            .get_push_record(&request.payload.push_id)
            .await
            .unwrap()
            .is_none(),
        "terminal SDK replay recreated a pruned projection"
    );
    assert!(
        native
            .runtime()
            .local_payload_is_compacted_for_test(reference)
            .await
            .unwrap()
    );
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

#[tokio::test]
async fn a_held_local_intent_keeps_its_frozen_item_and_link_until_the_reader_runs() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime_without_board().await;
    let (app, native) = native_application(&fixture, &runtime, installation_peer());
    let publisher = crate::NotificationPublisher::new(
        fixture.admission.clone(),
        fixture.interactions.clone(),
        Arc::downgrade(&native.runtime_handle()),
    );
    let intent = create_addressed_intent(&fixture).await;
    publisher
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    let held = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let target = target_session(&fixture.reader).unwrap();
            let held = native.interactions.list_held_mail_push_records(&target).await.unwrap();
            if let Some(record) = held.into_iter().find(|record| matches!(&record.header_facts, PushHeaderFacts::BoardNotification { intent: delivered, .. } if delivered == &intent.intent_id)) { break record; }
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    let frozen = frozen_item(&fixture, &intent).await;
    let frozen_value = serde_json::to_value(&frozen).unwrap();
    let reference = DeliveryItemRef {
        item: frozen.id(),
        digest: frozen.digest().clone(),
    };
    let first = app
        .messages()
        .push_show(PushRecordShowParams {
            caller: held.target.clone(),
            reference: held.push_id.as_str().to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(first.record.delivery_state, PushDeliveryState::Held);
    assert!(runtime.requests.lock().await.try_recv().is_err());
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
    await_compaction(&native, reference).await;
    let last = app
        .messages()
        .push_show(PushRecordShowParams {
            caller: held.target,
            reference: held.push_id.as_str().to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(last.link, first.link);
    assert_eq!(last.record.delivery_state, PushDeliveryState::Delivered);
    assert_eq!(
        serde_json::to_value(frozen_item(&fixture, &intent).await).unwrap(),
        frozen_value
    );
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
