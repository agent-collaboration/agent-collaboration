//! Real owner transactions and cold reopen; the receipt stands in for external native I/O.
use super::*;
use collaboration_protocol::{
    DeliveryClientReceipt, InteractionId, InteractionPresentationId, PushHeaderFacts, PushOrigin,
    PushRecordDraft, RouterOriginRef, SessionReachability,
};

fn notice_draft() -> PushRecordDraft {
    let target: collaboration_protocol::SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
        "sessionId":"approval-owner-recipient"
    }))
    .unwrap();
    let origin = RouterOriginRef::Interaction {
        interaction_id: InteractionId::try_from("approval-physical-owner".to_owned()).unwrap(),
        presentation_id: InteractionPresentationId::generate(),
    }
    .canonical_string()
    .unwrap();
    PushRecordDraft {
        push_id: uuid::Uuid::now_v7().to_string().try_into().unwrap(),
        kind: PushKind::Approval,
        origin: PushOrigin::Router(PushKind::Approval),
        origin_router_ref: Some(origin),
        target: target.clone(),
        mode: None,
        guard: None,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::Approval {
            requester: target,
            requester_display_name: None,
        },
        body: Some("Keep this complete approval notice".to_owned()),
        activity: None,
        created_at: Utc::now(),
    }
}
fn accepted_receipt() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::PeerMessageWritten,
        reachability: Some(SessionReachability::ClaudeCodePeer),
        client: Some(DeliveryClientReceipt::ClaudeCodePeer),
    }
}

#[tokio::test]
async fn notice_attempt_and_complete_native_receipt_survive_owner_reopen() {
    // Arrange: one physical store; no automation database or SDK custody is opened.
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("interaction.sqlite");
    let owner = InteractionStore::open(&path).await.unwrap();
    let draft = notice_draft();
    let push_id = draft.push_id.clone();
    let expected = accepted_receipt();
    owner.insert_push_record(draft.clone()).await.unwrap();
    let attempted = owner
        .mark_interaction_notice_attempted(&push_id)
        .await
        .unwrap();
    assert_eq!(attempted.delivery_state, PushDeliveryState::Attempted);
    assert!(attempted.last_outcome.is_none());
    // Act: settle the full external receipt and reopen the real file.
    let settled_at = Utc::now();
    owner
        .settle_interaction_notice(&push_id, expected.clone(), settled_at)
        .await
        .unwrap();
    owner.close().await.unwrap();
    let owner = InteractionStore::open(&path).await.unwrap();
    // Assert: the complete receipt, original push and first settlement remain unchanged.
    assert_eq!(
        owner
            .recover_interrupted_interaction_notices(Utc::now())
            .await
            .unwrap(),
        0
    );
    let stored = owner.get_push_record(&push_id).await.unwrap().unwrap();
    assert_eq!(stored.delivery_state, PushDeliveryState::Delivered);
    assert_eq!(
        serde_json::to_value(stored.last_outcome).unwrap(),
        serde_json::to_value(Some(expected)).unwrap()
    );
    assert_eq!(stored.settled_at, Some(settled_at));
    assert_eq!(stored.body, draft.body);
    assert!(!root.path().join("automation.sqlite").exists());
    owner.close().await.unwrap();
}

#[tokio::test]
async fn interrupted_notice_becomes_unknown_once_at_cold_reopen() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("interaction.sqlite");
    let owner = InteractionStore::open(&path).await.unwrap();
    let draft = notice_draft();
    let push_id = draft.push_id.clone();
    owner.insert_push_record(draft).await.unwrap();
    owner
        .mark_interaction_notice_attempted(&push_id)
        .await
        .unwrap();
    owner.close().await.unwrap();
    let owner = InteractionStore::open(&path).await.unwrap();
    let recovered_at = Utc::now();
    assert_eq!(
        owner
            .recover_interrupted_interaction_notices(recovered_at)
            .await
            .unwrap(),
        1
    );
    let first = owner.get_push_record(&push_id).await.unwrap().unwrap();
    assert_eq!(first.delivery_state, PushDeliveryState::OutcomeUnknown);
    assert_eq!(
        first.last_outcome.as_ref().unwrap().outcome,
        DeliveryOutcome::Unknown
    );
    assert_eq!(first.settled_at, Some(recovered_at));
    assert_eq!(
        owner
            .recover_interrupted_interaction_notices(Utc::now())
            .await
            .unwrap(),
        0
    );
    let second = owner.get_push_record(&push_id).await.unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(second).unwrap(),
        serde_json::to_value(first).unwrap()
    );
    owner.close().await.unwrap();
}

#[tokio::test]
async fn notice_transition_refuses_a_direct_message_without_changing_it() {
    let root = tempfile::tempdir().unwrap();
    let owner = InteractionStore::open(root.path().join("interaction.sqlite"))
        .await
        .unwrap();
    let mut draft = notice_draft();
    draft.kind = PushKind::DirectMessage;
    draft.origin = PushOrigin::OwnerUnverified;
    draft.origin_router_ref = None;
    draft.mode = Some(collaboration_protocol::MessageDelivery::Auto);
    draft.header_facts = PushHeaderFacts::DirectMessage {
        sender_display_name: None,
    };
    let push_id = draft.push_id.clone();
    owner.insert_push_record(draft).await.unwrap();
    let original = owner.get_push_record(&push_id).await.unwrap().unwrap();
    assert!(matches!(
        owner.mark_interaction_notice_attempted(&push_id).await,
        Err(StorageError::PushNotPermitted)
    ));
    assert!(matches!(
        owner
            .settle_interaction_notice(&push_id, accepted_receipt(), Utc::now())
            .await,
        Err(StorageError::PushNotPermitted)
    ));
    assert_eq!(
        owner
            .recover_interrupted_interaction_notices(Utc::now())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        serde_json::to_value(owner.get_push_record(&push_id).await.unwrap().unwrap()).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    owner.close().await.unwrap();
}

#[tokio::test]
async fn corrupt_later_notice_rolls_back_earlier_recovery_in_the_same_transaction() {
    let root = tempfile::tempdir().unwrap();
    let owner = InteractionStore::open(root.path().join("interaction.sqlite"))
        .await
        .unwrap();
    let mut drafts = vec![notice_draft(), notice_draft()];
    drafts.sort_by(|a, b| a.push_id.as_str().cmp(b.push_id.as_str()));
    let first = drafts.first().unwrap().push_id.clone();
    let later = drafts.last().unwrap().push_id.clone();
    assert_ne!(first, later);
    for draft in drafts {
        let push_id = draft.push_id.clone();
        owner.insert_push_record(draft).await.unwrap();
        owner
            .mark_interaction_notice_attempted(&push_id)
            .await
            .unwrap();
    }
    // Fault injection targets only an application-owned row, never SDK-private custody.
    sqlx::query("UPDATE router_pushes SET header_facts_json='{}' WHERE push_id=?")
        .bind(later.as_str())
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    assert!(
        owner
            .recover_interrupted_interaction_notices(Utc::now())
            .await
            .is_err()
    );
    let first = owner.get_push_record(&first).await.unwrap().unwrap();
    assert_eq!(first.delivery_state, PushDeliveryState::Attempted);
    assert!(first.last_outcome.is_none());
    assert!(first.settled_at.is_none());
    owner.close().await.unwrap();
}
