//! Physical commit/rollback and library saved-answer replay, not a native-provider observation.
#[path = "mail_delivery_cancellation_tests.rs"]
mod cancellation;
#[path = "mail_delivery_cancellation_storage_tests.rs"]
mod cancellation_storage;
#[path = "mail_delivery_projection_fixture.rs"]
pub(crate) mod fixture;
use super::*;
use collaboration_protocol::{
    DeliveryNextAction, DeliveryRejection, DeliveryRejectionReason, PushDeliveryState,
};
use fixture::*;
use fleet_sync_delivery::LocalOutcome;
use std::sync::atomic::Ordering;

fn rejection(detail: String) -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::Rejected(DeliveryRejection {
            native_failure: None,
            reason: DeliveryRejectionReason::LiveElsewhere,
            next_action: DeliveryNextAction::InspectTarget,
            client_code: Some(17),
            detail: Some(detail),
            claims: None,
        }),
        reachability: Some(collaboration_protocol::SessionReachability::ClaudeCodePeer),
        client: None,
    }
}

#[tokio::test]
async fn receive_hook_failure_rolls_back_custody_and_push_and_reopens_cleanly() {
    let fixture = Fixture::new().await;
    let draft = fixture.draft();
    fixture.sql("CREATE TRIGGER reject_received_push BEFORE INSERT ON router_pushes BEGIN SELECT RAISE(ABORT, 'fixture receive projection refused'); END").await;
    let error = fixture.receive(draft.clone()).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("fixture receive projection refused"),
        "{error:?}"
    );
    assert_eq!(fixture.counts().await, vec![0, 0, 0, 0]);
    fixture.sql("DROP TRIGGER reject_received_push").await;
    let fixture = fixture.reopen().await;
    fixture.receive(draft).await.unwrap();
    assert_eq!(fixture.counts().await, vec![1, 1, 0, 0]);
    drop(fixture.close().await);
}

#[tokio::test]
async fn outcome_and_recorded_answer_roll_back_together_and_replay_without_another_call() {
    let fixture = Fixture::new().await;
    let draft = fixture.draft();
    let push_id = draft.push_id.clone();
    let item_id = fixture.item(&draft).id();
    assert!(fixture.admit_local(draft).await.unwrap());
    let receipt = rejection("all original native detail and action retained".to_owned());
    fixture.push_answer(
        LocalOutcome::Refused {
            reason: fleet_sync_protocol::DeliveryRefusal::RecipientUnknown,
        },
        receipt.clone(),
    );
    fixture.sql("CREATE TRIGGER reject_mail_outcome BEFORE UPDATE ON router_pushes WHEN NEW.delivery_state<>'attempted' BEGIN SELECT RAISE(ABORT, 'fixture outcome projection refused'); END").await;
    let engine = fixture.engine();
    let report = engine.drive_due().await.unwrap();
    assert_eq!(report.retried.len(), 1, "{report:?}");
    assert_eq!(fixture.host.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture
            .owner
            .get_push_record(&push_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Attempted
    );
    assert!(fixture.local_outcome(item_id).await.is_none());
    let answer_id = item_id.as_uuid().to_string().try_into().unwrap();
    assert!(
        fixture
            .owner
            .message_send_answer(&answer_id)
            .await
            .unwrap()
            .unwrap()
            .result
            .is_none()
    );
    fixture.sql("DROP TRIGGER reject_mail_outcome").await;
    drop(engine);
    fixture.host.clock.fetch_add(100_000, Ordering::SeqCst);
    let engine = fixture.engine();
    let report = engine.drive_due().await.unwrap();
    assert_eq!(report.completed.len(), 1, "{report:?}");
    assert_eq!(
        fixture.host.calls.load(Ordering::SeqCst),
        1,
        "saved answer was dispatched again"
    );
    assert!(matches!(
        fixture.local_outcome(item_id).await,
        Some(RecipientOutcome::Refused { .. })
    ));
    let answer = fixture
        .owner
        .message_send_answer(&answer_id)
        .await
        .unwrap()
        .unwrap()
        .result
        .unwrap();
    assert_eq!(
        serde_json::to_value(local_answer(answer).receipt).unwrap(),
        serde_json::to_value(receipt).unwrap()
    );
    assert_eq!(fixture.counts().await, vec![1, 1, 0, 1]);
    drop(engine);
    drop(fixture.close().await);
}

#[tokio::test]
async fn oversized_real_task_answer_projects_uncertain_without_an_observation_or_resend() {
    let fixture = Fixture::new().await;
    let draft = fixture.draft();
    let push_id = draft.push_id.clone();
    let id = fixture.item(&draft).id();
    fixture.admit_local(draft).await.unwrap();
    fixture.push_answer(
        LocalOutcome::Refused {
            reason: fleet_sync_protocol::DeliveryRefusal::RecipientUnknown,
        },
        // Decision 939's declared bound, independent of the production constant.
        rejection("x".repeat(1024 * 1024 + 32 * 1024 + 1)),
    );
    let engine = fixture.engine();
    let report = engine.drive_due().await.unwrap();
    assert_eq!(report.completed.len(), 1, "{report:?}");
    assert!(matches!(
        fixture.local_outcome(id).await,
        Some(RecipientOutcome::Uncertain)
    ));
    let native = fixture
        .owner
        .get_push_record(&push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(native.delivery_state, PushDeliveryState::OutcomeUnknown);
    assert_eq!(
        native.last_outcome.unwrap().outcome,
        DeliveryOutcome::Unknown
    );
    let answer_id = id.as_uuid().to_string().try_into().unwrap();
    let answer = fixture
        .owner
        .message_send_answer(&answer_id)
        .await
        .unwrap()
        .unwrap()
        .result
        .unwrap();
    assert_eq!(
        local_answer(answer.clone()).receipt.outcome,
        DeliveryOutcome::Unknown
    );
    assert!(serde_json::to_vec(&answer).unwrap().len() < 1024);
    fixture.host.clock.fetch_add(100_000, Ordering::SeqCst);
    engine.drive_due().await.unwrap();
    assert_eq!(fixture.host.calls.load(Ordering::SeqCst), 1);
    drop(engine);
    drop(fixture.close().await);
}

#[tokio::test]
async fn compact_library_identity_replays_the_recorded_answer_after_push_pruning() {
    let fixture = Fixture::new().await;
    let draft = fixture.draft();
    let push_id = draft.push_id.clone();
    let id = fixture.item(&draft).id();
    fixture.admit_local(draft.clone()).await.unwrap();
    fixture.push_answer(
        LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::PeerMessageWritten,
        },
        DeliveryReceipt {
            outcome: DeliveryOutcome::PeerMessageWritten,
            reachability: Some(collaboration_protocol::SessionReachability::ClaudeCodePeer),
            client: None,
        },
    );
    let engine = fixture.engine();
    assert_eq!(engine.drive_due().await.unwrap().completed.len(), 1);
    drop(engine);
    let answer_id = id.as_uuid().to_string().try_into().unwrap();
    let original = fixture
        .owner
        .message_send_answer(&answer_id)
        .await
        .unwrap()
        .unwrap()
        .result
        .unwrap();
    assert_eq!(
        fixture
            .owner
            .prune_push_records(recorded_at() + chrono::Duration::days(31), 10)
            .await
            .unwrap(),
        1
    );
    assert!(
        fixture
            .owner
            .get_push_record(&push_id)
            .await
            .unwrap()
            .is_none()
    );
    let fixture = fixture.reopen().await;
    assert!(
        !fixture.admit_local(draft.clone()).await.unwrap(),
        "compacted item was admitted again"
    );
    let replay = fixture
        .owner
        .message_send_answer(&answer_id)
        .await
        .unwrap()
        .unwrap()
        .result
        .unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    let mut changed = draft.clone();
    changed.body = Some("different body under the old identity".to_owned());
    assert!(fixture.admit_local(changed).await.is_err());
    let mut fresh = draft;
    fresh.push_id = uuid::Uuid::now_v7().to_string().try_into().unwrap();
    assert!(
        fixture.admit_local(fresh).await.unwrap(),
        "new identity did not reach admission"
    );
    assert_eq!(
        fixture.host.calls.load(Ordering::SeqCst),
        0,
        "reopen replay called the carrier"
    );
    drop(fixture.close().await);
}

#[tokio::test]
async fn remote_outcome_and_owed_report_share_the_native_push_transaction() {
    let fixture = Fixture::new().await;
    let draft = fixture.draft();
    let push_id = draft.push_id.clone();
    let item = fixture.item(&draft).id();
    fixture.receive(draft).await.unwrap();
    fixture.push_answer(
        LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::PeerMessageWritten,
        },
        DeliveryReceipt {
            outcome: DeliveryOutcome::PeerMessageWritten,
            reachability: Some(collaboration_protocol::SessionReachability::ClaudeCodePeer),
            client: None,
        },
    );
    fixture.sql("CREATE TRIGGER reject_remote_outcome BEFORE UPDATE ON router_pushes WHEN NEW.delivery_state<>'attempted' BEGIN SELECT RAISE(ABORT, 'fixture remote outcome refused'); END").await;
    let engine = fixture.engine();
    let report = engine.drive_due().await.unwrap();
    assert_eq!(report.retried.len(), 1, "{report:?}");
    assert_eq!(fixture.counts().await, vec![1, 1, 0, 0]);
    assert!(fixture.local_outcome(item).await.is_none());
    assert_eq!(
        fixture
            .owner
            .get_push_record(&push_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Attempted
    );
    fixture.sql("DROP TRIGGER reject_remote_outcome").await;
    fixture.host.clock.fetch_add(100_000, Ordering::SeqCst);
    assert_eq!(engine.drive_due().await.unwrap().completed.len(), 1);
    assert_eq!(fixture.counts().await, vec![1, 1, 1, 0]);
    assert!(matches!(
        fixture.report(item).await,
        Some(RecipientOutcome::Delivered {
            acceptance: CarrierAcceptance::PeerMessageWritten
        })
    ));
    assert_eq!(fixture.host.calls.load(Ordering::SeqCst), 1);
    drop(engine);
    drop(fixture.close().await);
}

#[tokio::test]
async fn too_late_cancel_marks_target_without_rewriting_receipt_answer_or_first_settlement() {
    let fixture = Fixture::new().await;
    let draft = fixture.draft();
    let push_id = draft.push_id.clone();
    let id = fixture.item(&draft).id();
    fixture.admit_local(draft.clone()).await.unwrap();
    fixture.push_answer(
        LocalOutcome::Delivered {
            acceptance: CarrierAcceptance::PeerMessageWritten,
        },
        DeliveryReceipt {
            outcome: DeliveryOutcome::PeerMessageWritten,
            reachability: Some(collaboration_protocol::SessionReachability::ClaudeCodePeer),
            client: None,
        },
    );
    let engine = fixture.engine();
    assert_eq!(engine.drive_due().await.unwrap().completed.len(), 1);
    drop(engine);
    let before = fixture
        .owner
        .get_push_record(&push_id)
        .await
        .unwrap()
        .unwrap();
    let answer_id = id.as_uuid().to_string().try_into().unwrap();
    let answer = fixture
        .owner
        .message_send_answer(&answer_id)
        .await
        .unwrap()
        .unwrap()
        .result
        .unwrap();
    // Decision614 permits only the new metadata column to change.
    fixture.sql("CREATE TRIGGER reject_terminal_push_rewrite BEFORE UPDATE OF push_id,retention,kind,origin_kind,origin_service_id,origin_endpoint_id,origin_session_id,origin_router_ref,target_service_id,target_endpoint_id,target_session_id,reply_to_push_id,header_facts_json,body,ranges_json,delivery_state,last_outcome_json,created_at,settled_at,read_at,dm_delivery_mode,dm_generation_guard_json ON router_pushes BEGIN SELECT RAISE(ABORT, 'terminal native push rewritten'); END").await;
    fixture.sql("CREATE TRIGGER reject_terminal_answer_rewrite BEFORE UPDATE ON message_send_answers BEGIN SELECT RAISE(ABORT, 'terminal RPC answer rewritten'); END").await;
    fixture.host.clock.fetch_add(100_000, Ordering::SeqCst);
    let cancel = fixture.receive_cancel(&draft).await;
    assert!(matches!(
        fixture.report(cancel).await,
        Some(RecipientOutcome::TooLate {
            at: fleet_sync_protocol::TooLateAt::Delivered,
            ..
        })
    ));
    let after = fixture
        .owner
        .get_push_record(&push_id)
        .await
        .unwrap()
        .unwrap();
    let mut after = serde_json::to_value(after).unwrap();
    let mark = after
        .as_object_mut()
        .unwrap()
        .remove("cancelledAfterDelivery");
    assert_eq!(
        mark,
        Some(serde_json::json!({
            "cancel":cancel,"tooLateAt":"delivered","recordedAt":"2026-10-19T18:01:40Z",
        }))
    );
    assert_eq!(after, serde_json::to_value(before).unwrap());
    assert_eq!(
        serde_json::to_value(
            fixture
                .owner
                .message_send_answer(&answer_id)
                .await
                .unwrap()
                .unwrap()
                .result
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(answer).unwrap()
    );
    assert_eq!(fixture.host.calls.load(Ordering::SeqCst), 1);
    fixture
        .sql("DROP TRIGGER reject_terminal_push_rewrite")
        .await;
    fixture
        .sql("DROP TRIGGER reject_terminal_answer_rewrite")
        .await;
    drop(fixture.close().await);
}

fn local_answer(result: MessageSendResult) -> PushMessageSendResult {
    match result {
        MessageSendResult::Local(receipt) => receipt,
        MessageSendResult::Queued { .. } => panic!("local delivery recorded remote sender custody"),
    }
}

#[tokio::test]
async fn a_remote_reply_link_survives_shared_wire_native_receipt_and_owning_reopen() {
    // Arrange: these are different installations; the literal is the original
    // item's identity, independent of the reply's freshly minted item identity.
    let fixture = Fixture::new().await;
    let original: collaboration_protocol::PushId = "01900000-0000-7000-8000-000000000725"
        .to_owned()
        .try_into()
        .unwrap();
    let mut reply = fixture.draft();
    let reply_id = reply.push_id.clone();
    reply.reply_to_push_id = Some(original.clone());
    reply.origin = collaboration_protocol::PushOrigin::Session(
        serde_json::from_value(serde_json::json!({
            "endpoint": {
                "serviceId": "00000000-0000-4000-8000-000000000002",
                "endpointId": "codex-local"
            },
            "sessionId": "reply-sender"
        }))
        .unwrap(),
    );
    let sender = "00000000-0000-4000-8000-000000000002"
        .to_owned()
        .try_into()
        .unwrap();
    let item =
        crate::outgoing_mail_message(&reply, &sender, fixture.host.config.local.owner).unwrap();
    let reply_digest = item.digest().clone();
    let shared: fleet_sync_protocol::DeliveryItem<board_sync_protocol::BoardDeliveryTypes> =
        serde_json::from_slice(&serde_json::to_vec(&item).unwrap()).unwrap();
    let received: fleet_sync_protocol::DeliveryItem<crate::RouterDeliveryTypes> =
        serde_json::from_slice(&serde_json::to_vec(&shared).unwrap()).unwrap();

    // Act: the actual Inbox receipt invokes the real native receive hook in
    // its transaction. Close every owning native handle before the readback.
    fixture.receive_items(vec![received]).await.unwrap();
    let fixture = fixture.reopen().await;

    // Assert: dropping the link at either edge changes this durable result.
    let stored = fixture
        .owner
        .get_push_record(&reply_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.reply_to_push_id,
        Some(original),
        "remote mail discarded the original item's reply link",
    );
    reply.reply_to_push_id = None;
    let ordinary =
        crate::outgoing_mail_message(&reply, &sender, fixture.host.config.local.owner).unwrap();
    assert_ne!(
        reply_digest,
        *ordinary.digest(),
        "reply link was outside the item digest"
    );
    assert!(
        crate::received_mail_push(&ordinary, recorded_at())
            .unwrap()
            .unwrap()
            .reply_to_push_id
            .is_none()
    );
    drop(fixture.close().await);
}
