//! Real C5 owner coordination; only the external native client is scripted.
use super::super::SubscriptionClock;
use super::super::subscription_push::target_session;
use super::owner_fixture::*;
use crate as mail_service;
use crate::ApplicationFixture;
use crate::CollaborationApplication;
use collaboration_contracts::TargetPresence;
#[path = "follow_admission_proofs.rs"]
mod follow_admission_proofs;
#[path = "incoming_poll_session.rs"]
mod incoming_poll_session;
#[path = "membership_expiry_admission_proofs.rs"]
mod membership_expiry_admission_proofs;
#[path = "native_cancel_backpressure_proofs.rs"]
mod native_cancel_backpressure_proofs;
#[path = "native_cancel_takeover_fixture.rs"]
mod native_cancel_takeover_fixture;
#[path = "native_follow_cancel_proofs.rs"]
mod native_follow_cancel_proofs;
#[path = "native_follow_expiry_proofs.rs"]
mod native_follow_expiry_proofs;
#[path = "native_follow_grouping_proofs.rs"]
mod native_follow_grouping_proofs;
#[path = "native_follow_injection_proofs.rs"]
mod native_follow_injection_proofs;
#[path = "native_follow_mail_proofs.rs"]
pub(super) mod native_follow_mail_proofs;
#[path = "native_follow_outcome_fault_proofs.rs"]
mod native_follow_outcome_fault_proofs;
#[path = "native_frontier_proofs.rs"]
mod native_frontier_proofs;
#[path = "native_generation_isolation_proofs.rs"]
mod native_generation_isolation_proofs;
#[path = "native_generation_prevention_proofs.rs"]
mod native_generation_prevention_proofs;
#[path = "native_handoff_recovery_proofs.rs"]
mod native_handoff_recovery_proofs;
#[path = "native_held_mail_proofs.rs"]
mod native_held_mail_proofs;
#[path = "native_intent_publisher_proofs.rs"]
mod native_intent_publisher_proofs;
#[path = "../../../tests/support/native_mail_owner.rs"]
#[expect(
    dead_code,
    reason = "The shared fixture also exposes deferred worker startup used by Host takeover tests."
)]
pub(super) mod native_mail_owner;
#[path = "native_owed_transfer_proofs.rs"]
mod native_owed_transfer_proofs;
#[path = "native_poll_gate_proofs.rs"]
mod native_poll_gate_proofs;
#[path = "native_poll_incoming_proofs.rs"]
mod native_poll_incoming_proofs;
#[path = "native_poll_mail_proofs.rs"]
mod native_poll_mail_proofs;
#[path = "native_poll_recovery_proofs.rs"]
mod native_poll_recovery_proofs;
#[path = "native_readiness_interleaving_proofs.rs"]
mod native_readiness_interleaving_proofs;
#[path = "native_successor_frontier_proofs.rs"]
mod native_successor_frontier_proofs;
#[path = "native_window_lineage_proofs.rs"]
mod native_window_lineage_proofs;
#[path = "window_lineage_fixture.rs"]
mod window_lineage_fixture;
use collaboration_protocol::{
    DeliveryOutcome, MailItemId, MessageContent, MessageDelivery, PushDeliveryState,
    PushHeaderFacts, PushKind, PushOrigin, PushRecordDraft, PushRecordShowParams,
    SessionMessageSendParams,
};
use fleet_sync_protocol::{DeliveryItemRef, PeerId};
use native_mail_owner::NativeMailOwner;
use std::sync::Arc;
use std::time::Duration;

fn native_application(
    fixture: &OwnerFixture,
    runtime: &OwnerRuntime,
    peer: PeerId,
) -> (Arc<CollaborationApplication>, NativeMailOwner) {
    let interactions = fixture.interactions.clone();
    let (identity, owner) = NativeMailOwner::start(
        ApplicationFixture::new(SERVICE_ID, SERVICE_ID)
            .unwrap()
            .with_automation_store(fixture.automation.clone())
            .with_subscription_delivery_service(runtime.service.clone(), fixture.presence.clone()),
        peer,
        crate::MailPushDeliveryProps {
            interactions,
            actors: runtime.service.clone(),
            presence: fixture.presence.clone(),
            machine: collaboration_contracts::MachineIdentity::new(
                SERVICE_ID.to_owned().try_into().unwrap(),
                None,
            )
            .unwrap(),
            clock: fixture.clock.clone(),
        },
    )
    .unwrap();
    (Arc::new((identity).build()), owner)
}

fn reference_for_request(
    input: &SessionMessageSendParams,
    push: &collaboration_protocol::PushId,
    peer: PeerId,
) -> DeliveryItemRef {
    let (origin, text) = match &input.message {
        MessageContent::Agent { sender, text } => {
            (PushOrigin::Session(sender.clone()), text.as_str())
        }
        MessageContent::HumanUser { text } => (PushOrigin::OwnerUnverified, text.as_str()),
        MessageContent::Router { .. } => {
            panic!("local lifecycle proof requires caller-authored content")
        }
    };
    let draft = PushRecordDraft {
        push_id: push.clone(),
        kind: PushKind::DirectMessage,
        origin,
        origin_router_ref: None,
        target: input.target.clone(),
        mode: Some(input.mode),
        guard: input.generation_guard.clone(),
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        },
        body: Some(text.to_owned()),
        activity: None,
        created_at: chrono::Utc::now(),
    };
    let item = collaboration_runtime::outgoing_mail_message(
        &draft,
        &SERVICE_ID.to_owned().try_into().unwrap(),
        peer,
    )
    .unwrap();
    DeliveryItemRef {
        item: item.id(),
        digest: item.digest().clone(),
    }
}

async fn await_native_completion(runtime: &OwnerRuntime, count: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while runtime
            .completed_native_calls
            .load(std::sync::atomic::Ordering::SeqCst)
            < count
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("scripted native client did not return its offered completion");
    assert_eq!(
        runtime
            .completed_native_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        count
    );
}

async fn await_compaction(owner: &NativeMailOwner, reference: DeliveryItemRef) {
    let compacted = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if owner
                .runtime()
                .local_payload_is_compacted_for_test(reference.clone())
                .await
                .unwrap()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let push = reference.item.as_uuid().to_string().try_into().unwrap();
    assert!(
        compacted.is_ok(),
        "native outcome did not commit and compact its executable payload: {:?}",
        owner.interactions.get_push_record(&push).await.unwrap()
    );
}

#[tokio::test]
async fn concurrent_same_item_reports_pending_then_replays_the_exact_settled_outcome() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
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
    let submitted = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    assert_eq!(submitted.payload.push_id.as_str(), id.as_str());
    let second = tokio::time::timeout(
        Duration::from_secs(10),
        app.messages().message_send(input.clone()),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(
        second.kind,
        crate::collaboration_application::MessageFailureKind::OutcomeUnknown
    );
    assert!(runtime.requests.lock().await.try_recv().is_err());
    let stored = native
        .interactions
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
    let replay = app.messages().message_send(input.clone()).await.unwrap();
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&replay).unwrap()
    );
    await_compaction(
        &native,
        reference_for_request(&input, &submitted.payload.push_id, peer),
    )
    .await;
    runtime.service.shutdown().await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}

#[tokio::test]
async fn aged_held_mail_survives_store_reopen_and_delivers_full_content_through_the_same_owner() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let target = target_session(&fixture.reader).unwrap();
    let id = MailItemId::generate();
    let body = "A complete long message that requires show beyond the preview. ".repeat(40);
    let draft = PushRecordDraft {
        push_id: id.clone().try_into().unwrap(),
        kind: PushKind::DirectMessage,
        origin: PushOrigin::OwnerUnverified,
        origin_router_ref: None,
        target: target.clone(),
        mode: Some(MessageDelivery::Queue),
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
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let item = collaboration_runtime::outgoing_mail_message(
        &draft,
        &SERVICE_ID.to_owned().try_into().unwrap(),
        peer,
    )
    .unwrap();
    let reference = DeliveryItemRef {
        item: item.id(),
        digest: item.digest().clone(),
    };
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let initial = fixture.runtime_without_board().await;
    let (app, native) = native_application(&fixture, &initial, peer);
    let held = tokio::time::timeout(
        Duration::from_secs(10),
        native.runtime().send_local(draft, "fixture reader".into()),
    )
    .await
    .expect("loaded-only queue did not enter held custody")
    .unwrap();
    assert_eq!(held.delivery_state, PushDeliveryState::Held);
    assert_eq!(
        native
            .interactions
            .prune_push_records(fixture.clock.now(), 500)
            .await
            .unwrap(),
        0
    );
    assert!(initial.requests.lock().await.try_recv().is_err());
    drop(app);
    initial.close().await;
    let fixture = fixture.reopen_native(native).await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
    let restarted = fixture.runtime_without_board().await;
    let (app, native) = native_application(&fixture, &restarted, peer);
    let reopened = native
        .interactions
        .get_push_record(&push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.body.as_deref(), Some(body.as_str()));
    assert_eq!(reopened.delivery_state, PushDeliveryState::Held);
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        restarted.requests.lock().await.recv().await.unwrap()
    })
    .await
    .expect("reopened native owner did not resume its held local mail");
    assert_eq!(request.payload.push_id, push_id);
    restarted
        .completions
        .send(DeliveryOutcome::Started)
        .unwrap();
    await_native_completion(&restarted, 1).await;
    await_compaction(&native, reference).await;
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
        native
            .interactions
            .list_pending_push_records(&target)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        native
            .interactions
            .list_held_push_records(&target)
            .await
            .unwrap()
            .is_empty()
    );
    restarted.service.shutdown().await;
    assert!(restarted.requests.lock().await.try_recv().is_err());
    drop(app);
    restarted.close().await;
    fixture.close_native(native).await;
}

#[tokio::test]
async fn disconnected_sender_waiter_keeps_custody_and_only_an_omitted_id_creates_new_work() {
    for supplied in [true, false] {
        let fixture = Box::pin(OwnerFixture::new()).await;
        *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
        let runtime = fixture.runtime_without_board().await;
        let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
        let (app, native) = native_application(&fixture, &runtime, peer);
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
        await_native_completion(&runtime, 1).await;
        await_compaction(&native, reference_for_request(&input, &original_id, peer)).await;
        if supplied {
            let replay = app.messages().message_send(input).await.unwrap();
            let collaboration_protocol::MessageSendResult::Local(replay) = replay else {
                panic!("local replay returned remote sender custody");
            };
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
            let collaboration_protocol::MessageSendResult::Local(result) = result else {
                panic!("new local send returned remote sender custody");
            };
            assert_eq!(result.push_id, new_submission.payload.push_id);
        }
        assert_eq!(
            runtime
                .completed_native_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            if supplied { 1 } else { 2 }
        );
        runtime.service.shutdown().await;
        assert!(runtime.requests.lock().await.try_recv().is_err());
        drop(app);
        runtime.close().await;
        fixture.close_native(native).await;
    }
}

#[tokio::test]
async fn local_native_attempt_is_drained_when_runtime_shutdown_starts() {
    // Arrange: keep the actual native client blocked after its stored attempt.
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.runtime_without_board().await;
    let peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let (app, native) = native_application(&fixture, &runtime, peer);
    let target = target_session(&fixture.reader).unwrap();
    let input = SessionMessageSendParams {
        item_id: Some(MailItemId::generate()),
        target: target.clone(),
        message: MessageContent::HumanUser {
            text: "finish accepted work before shutdown"
                .to_owned()
                .try_into()
                .unwrap(),
        },
        mode: MessageDelivery::Auto,
        generation_guard: None,
    };
    let first_app = app.clone();
    let first_input = input.clone();
    let first = tokio::spawn(async move { first_app.messages().message_send(first_input).await });
    let submitted = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    // Act: stop the real runtime while the actual external effect is unresolved.
    native.begin_shutdown();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !native
            .runtime()
            .settlement_admission_closed_for_test()
            .unwrap()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        !native.is_stopped(),
        "runtime abandoned an accepted native call"
    );
    let mut refused = input.clone();
    let refused_id = MailItemId::generate();
    refused.item_id = Some(refused_id.clone());
    let error = tokio::time::timeout(
        Duration::from_secs(10),
        app.messages().message_send(refused),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(
        error.kind,
        crate::collaboration_application::MessageFailureKind::Unavailable
    );
    assert!(
        native
            .interactions
            .get_push_record(&refused_id.try_into().unwrap())
            .await
            .unwrap()
            .is_none()
    );
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    let answer = tokio::time::timeout(Duration::from_secs(10), first)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let collaboration_protocol::MessageSendResult::Local(answer) = answer else {
        panic!("local shutdown drain returned remote custody");
    };
    // Assert: the original outcome commits and its executable payload is compacted.
    assert_eq!(answer.delivery_state, PushDeliveryState::Delivered);
    assert_eq!(answer.push_id, submitted.payload.push_id);
    await_compaction(
        &native,
        reference_for_request(&input, &answer.push_id, peer),
    )
    .await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}
