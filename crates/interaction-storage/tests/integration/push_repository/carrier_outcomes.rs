//! Existing carrier outcome and immutable snapshot oracles at the native owner.
use super::*;
use collaboration_protocol::{
    DeliveryClientReceipt, DeliveryOutcome, DeliveryReceipt, PushDeliveryState, SessionReachability,
};

#[tokio::test]
async fn board_notification_is_stored_once_and_interrupted_delivery_becomes_unknown() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("automation store");
    let project = message_board::ProjectId::generate();
    let intent = message_board::NotifyIntentId::generate();
    let draft = PushRecordDraft {
        push_id: push_id(),
        kind: PushKind::BoardNotification,
        origin: PushOrigin::Router(PushKind::BoardNotification),
        origin_router_ref: Some(
            RouterOriginRef::BoardIntent {
                project: project.clone(),
                intent: intent.clone(),
            }
            .canonical_string()
            .expect("board source"),
        ),
        target: session("codex-local", "board-recipient"),
        mode: None,
        guard: None,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::BoardNotification {
            project,
            intent,
            cause: message_board::RoleAssignmentId::generate().into(),
            summary: collaboration_protocol::MessageText::try_from("Role assigned".to_owned())
                .expect("summary"),
        },
        body: None,
        activity: None,
        created_at: Utc::now(),
    };
    let record = store
        .insert_push_record(draft.clone())
        .await
        .expect("stored board push");
    let mut duplicate = draft;
    duplicate.push_id = push_id();
    assert!(
        store.insert_push_record(duplicate).await.is_err(),
        "one push per claimed intent source"
    );
    store
        .mark_push_attempted(&record.push_id)
        .await
        .expect("attempt");
    assert_eq!(
        store
            .settle_interrupted_pushes(Utc::now())
            .await
            .expect("recovery"),
        1
    );
    let recovered = store
        .get_push_record(&record.push_id)
        .await
        .expect("read")
        .expect("saved push");
    assert_eq!(recovered.delivery_state, PushDeliveryState::OutcomeUnknown);
    assert_eq!(
        recovered.last_outcome.expect("unknown receipt").outcome,
        DeliveryOutcome::Unknown
    );
    store.close().await.expect("close");
}

#[tokio::test]
async fn inserted_direct_message_is_a_store_first_immutable_snapshot() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("automation store migrates");
    let sender = session("claude-local", "sender-thread");
    let target = session("codex-local", "target-thread");
    let push_id = push_id();
    let created_at = Utc::now() - Duration::seconds(1);
    let draft = push_draft(
        push_id.clone(),
        target.clone(),
        PushOrigin::Session(sender.clone()),
        "first snapshot",
        created_at,
    );

    let inserted = store
        .insert_push_record(draft.clone())
        .await
        .expect("insert before delivery");
    assert_eq!(inserted.delivery_state, PushDeliveryState::Pending);
    assert_eq!(inserted.body.as_deref(), Some("first snapshot"));
    assert_eq!(inserted.origin, PushOrigin::Session(sender.clone()));

    store
        .mark_push_attempted(&push_id)
        .await
        .expect("record the delivery attempt");
    let receipt = DeliveryReceipt {
        outcome: DeliveryOutcome::PeerMessageWritten,
        reachability: Some(SessionReachability::ClaudeCodePeer),
        client: Some(DeliveryClientReceipt::ClaudeCodePeer),
    };
    let settled = store
        .settle_push_record(&push_id, receipt, Utc::now())
        .await
        .expect("settle the accepted delivery");
    assert_eq!(settled.delivery_state, PushDeliveryState::Delivered);

    let conflicting_draft = push_draft(
        push_id.clone(),
        target,
        PushOrigin::Session(sender),
        "replacement text must not overwrite the snapshot",
        created_at,
    );
    assert!(matches!(
        store.insert_push_record(conflicting_draft).await,
        Err(StorageError::PushAlreadyExists)
    ));
    let read_back = store
        .get_push_record(&push_id)
        .await
        .expect("read stored push")
        .expect("push remains stored");
    assert_eq!(read_back.body.as_deref(), Some("first snapshot"));
    assert_eq!(read_back.delivery_state, PushDeliveryState::Delivered);
    store.close().await.expect("close automation store");
}

#[tokio::test]
async fn unknown_delivery_outcome_is_settled_and_not_pending_for_replay() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("automation store migrates");
    let sender = session("claude-local", "sender-thread");
    let target = session("codex-local", "target-thread");
    let push_id = push_id();
    store
        .insert_push_record(push_draft(
            push_id.clone(),
            target.clone(),
            PushOrigin::Session(sender),
            "reconcile once",
            Utc::now(),
        ))
        .await
        .expect("store before attempt");
    store
        .mark_push_attempted(&push_id)
        .await
        .expect("record attempted state");
    let settled = store
        .settle_push_record(
            &push_id,
            DeliveryReceipt {
                outcome: DeliveryOutcome::Unknown,
                reachability: None,
                client: None,
            },
            Utc::now(),
        )
        .await
        .expect("settle outcome unknown");

    assert_eq!(settled.delivery_state, PushDeliveryState::OutcomeUnknown);
    assert!(
        store
            .list_pending_push_records(&target)
            .await
            .expect("list pending")
            .is_empty()
    );
    assert!(
        store
            .list_held_push_records(&target)
            .await
            .expect("list held")
            .is_empty()
    );
    store.close().await.expect("close automation store");
}
