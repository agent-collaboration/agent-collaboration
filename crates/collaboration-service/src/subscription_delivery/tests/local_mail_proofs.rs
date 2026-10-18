//! Real C5 owner coordination; only the external native client is scripted.
use super::super::SubscriptionClock;
use super::super::{subscription_push::target_session, subscription_service::OwnerObservation};
use super::owner_fixture::*;
use crate::{CollaborationApplication, ServiceIdentity, TargetPresence};
use collaboration_protocol::{
    DeliveryOutcome, MailItemId, MessageContent, MessageDelivery, PushDeliveryState,
    PushHeaderFacts, PushKind, PushOrigin, PushRecordDraft, PushRecordShowParams,
    SessionMessageSendParams,
};
use std::sync::Arc;
use std::time::Duration;

async fn observe(runtime: &OwnerRuntime, matches: impl Fn(&OwnerObservation) -> bool) {
    tokio::time::timeout(Duration::from_secs(10), runtime.observe(matches))
        .await
        .unwrap();
}
fn application(fixture: &OwnerFixture, runtime: &OwnerRuntime) -> Arc<CollaborationApplication> {
    Arc::new(CollaborationApplication::new(
        ServiceIdentity::new(SERVICE_ID, SERVICE_ID)
            .unwrap()
            .with_automation_store(fixture.push_store.clone())
            .with_subscription_delivery_service(runtime.service.clone(), fixture.presence.clone()),
    ))
}

#[tokio::test]
async fn concurrent_same_item_waits_for_the_existing_attempt_and_returns_its_exact_outcome() {
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    let runtime = fixture.runtime_without_board().await;
    let app = application(&fixture, &runtime);
    let id = MailItemId::generate();
    let target = target_session(&fixture.reader).unwrap();
    let input = SessionMessageSendParams {
        item_id: Some(id.clone()),
        target: target.clone(),
        message: MessageContent::Agent {
            sender: target,
            text: "same in-flight item".to_owned().try_into().unwrap(),
        },
        mode: MessageDelivery::Auto,
        generation_guard: None,
    };
    let first_app = app.clone();
    let first_input = input.clone();
    let first = tokio::spawn(async move { first_app.messages().message_send(first_input).await });
    observe(&runtime, |event| {
        matches!(event, OwnerObservation::DirectMessageQueued)
    })
    .await;
    let submitted = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    assert_eq!(submitted.payload.push_id.as_str(), id.as_str());
    let second_app = app.clone();
    let second = tokio::spawn(async move { second_app.messages().message_send(input).await });
    // Existing owner acknowledgement proves the second command is queued while
    // the first external effect is still blocked, rather than assuming timing.
    observe(&runtime, |event| {
        matches!(event, OwnerObservation::DirectMessageQueued)
    })
    .await;
    let stored = fixture
        .push_store
        .lock()
        .await
        .get_push_record(&submitted.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.delivery_state, PushDeliveryState::Attempted);
    assert!(stored.settled_at.is_none());
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    let first = tokio::time::timeout(Duration::from_secs(10), first)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let second = tokio::time::timeout(Duration::from_secs(10), second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
    runtime.service.shutdown().await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    assert!(
        fixture
            .push_store
            .lock()
            .await
            .read_local_mail(&id)
            .await
            .unwrap()
            .unwrap()
            .compacted
    );
    drop(app);
    runtime.close().await;
}

#[tokio::test]
async fn aged_held_mail_survives_store_reopen_and_delivers_full_content_through_the_same_owner() {
    let fixture = OwnerFixture::new().await;
    let target = target_session(&fixture.reader).unwrap();
    let id = MailItemId::generate();
    let body = "A complete long message that requires show beyond the preview. ".repeat(40);
    let draft = PushRecordDraft {
        push_id: id.clone().try_into().unwrap(),
        kind: PushKind::DirectMessage,
        origin: PushOrigin::OwnerUnverified,
        origin_router_ref: None,
        target: target.clone(),
        mode: Some(MessageDelivery::Auto),
        guard: None,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        },
        body: Some(body.clone()),
        activity: None,
        created_at: fixture.clock.now() - chrono::Duration::days(40),
    };
    let push_id = draft.push_id.clone();
    fixture
        .push_store
        .lock()
        .await
        .prepare_local_message(&automation_storage::LocalMessageRequest {
            draft,
            target_identity: "fixture reader".into(),
        })
        .await
        .unwrap();
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let initial = fixture.runtime_without_board().await;
    observe(&initial, |event| {
        matches!(event, OwnerObservation::DirectMessageHeld)
    })
    .await;
    assert_eq!(
        fixture
            .push_store
            .lock()
            .await
            .prune_push_records(fixture.clock.now(), 500)
            .await
            .unwrap(),
        0
    );
    assert!(initial.requests.lock().await.try_recv().is_err());
    initial.close().await;
    let replacement = automation_storage::AutomationStore::open(
        &fixture.directory.path().join("automation.sqlite"),
    )
    .await
    .unwrap();
    let old = std::mem::replace(&mut *fixture.push_store.lock().await, replacement);
    old.close().await.unwrap();
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    let restarted = fixture.runtime_without_board().await;
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        restarted.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    assert_eq!(request.payload.push_id, push_id);
    restarted
        .completions
        .send(DeliveryOutcome::Started)
        .unwrap();
    observe(&restarted, |event| {
        matches!(event, OwnerObservation::DirectMessageSettled)
    })
    .await;
    let app = application(&fixture, &restarted);
    let shown = app
        .messages()
        .push_show(PushRecordShowParams {
            caller: target.clone(),
            reference: push_id.as_str().to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(shown.record.body.as_deref(), Some(body.as_str()));
    assert_eq!(
        shown.record.created_at,
        fixture.clock.now() - chrono::Duration::days(40)
    );
    assert_eq!(shown.record.settled_at, Some(fixture.clock.now()));
    assert_eq!(shown.record.delivery_state, PushDeliveryState::Delivered);
    assert!(shown.link.ends_with(push_id.as_str()));
    assert!(
        fixture
            .push_store
            .lock()
            .await
            .list_unsent_direct_messages(&target)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        fixture
            .push_store
            .lock()
            .await
            .read_local_mail(&id)
            .await
            .unwrap()
            .unwrap()
            .compacted
    );
    restarted.service.shutdown().await;
    assert!(restarted.requests.lock().await.try_recv().is_err());
    drop(app);
    restarted.close().await;
}

#[tokio::test]
async fn disconnected_sender_waiter_keeps_custody_and_only_an_omitted_id_creates_new_work() {
    for supplied in [true, false] {
        let fixture = OwnerFixture::new().await;
        *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
        let runtime = fixture.runtime_without_board().await;
        let app = application(&fixture, &runtime);
        let target = target_session(&fixture.reader).unwrap();
        let input = SessionMessageSendParams {
            item_id: supplied.then(MailItemId::generate),
            target: target.clone(),
            message: MessageContent::Agent {
                sender: target,
                text: "a disconnected caller".to_owned().try_into().unwrap(),
            },
            mode: MessageDelivery::Auto,
            generation_guard: None,
        };
        let first_app = app.clone();
        let first_input = input.clone();
        let first =
            tokio::spawn(async move { first_app.messages().message_send(first_input).await });
        let submission = tokio::time::timeout(Duration::from_secs(10), async {
            runtime.requests.lock().await.recv().await.unwrap()
        })
        .await
        .unwrap();
        let original_id = submission.payload.push_id;
        // This is the caller-waiter boundary, not a simulated successful return:
        // disconnect it while the real owner already holds the attempted input.
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        runtime.completions.send(DeliveryOutcome::Started).unwrap();
        observe(&runtime, |event| {
            matches!(event, OwnerObservation::DirectMessageSettled)
        })
        .await;
        let original_mail: MailItemId = original_id.clone().try_into().unwrap();
        assert!(
            fixture
                .push_store
                .lock()
                .await
                .read_local_mail(&original_mail)
                .await
                .unwrap()
                .unwrap()
                .compacted
        );
        if supplied {
            let replay = app.messages().message_send(input).await.unwrap();
            assert_eq!(replay.push_id, original_id);
            assert_eq!(replay.delivery_state, PushDeliveryState::Delivered);
        } else {
            let retry_app = app.clone();
            let retry = tokio::spawn(async move { retry_app.messages().message_send(input).await });
            let new_submission = tokio::time::timeout(Duration::from_secs(10), async {
                runtime.requests.lock().await.recv().await.unwrap()
            })
            .await
            .unwrap();
            assert_ne!(new_submission.payload.push_id, original_id);
            runtime.completions.send(DeliveryOutcome::Started).unwrap();
            let result = tokio::time::timeout(Duration::from_secs(10), retry)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(result.push_id, new_submission.payload.push_id);
        }
        runtime.service.shutdown().await;
        assert!(runtime.requests.lock().await.try_recv().is_err());
        drop(app);
        runtime.close().await;
    }
}
