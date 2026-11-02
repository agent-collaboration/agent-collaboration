//! Real Turso transactions preserve listing membership, cooldowns and uncertainty on reopen.
use crate::{InteractionStore, StorageError};
use chrono::{DateTime, Utc};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushDeliveryState, PushHeaderFacts, PushId, PushKind,
    PushOrigin, PushRecordDraft, RouterOriginRef, SessionRef,
};

fn at(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap()
}
fn target(session: &str) -> SessionRef {
    serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":session})).unwrap()
}
fn receipt(outcome: DeliveryOutcome) -> DeliveryReceipt {
    DeliveryReceipt {
        outcome,
        reachability: None,
        client: None,
    }
}
fn expiry(recipient: &SessionRef) -> PushRecordDraft {
    let scope = message_board::SubscriptionScope::thread(message_board::ThreadId::generate());
    PushRecordDraft {
        push_id: PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap(),
        kind: PushKind::SubscriptionExpiry,
        origin: PushOrigin::Router(PushKind::SubscriptionExpiry),
        origin_router_ref: Some(
            RouterOriginRef::SubscriptionExpiry {
                target: recipient.clone(),
                scope,
                subscription_generation: message_board::SubscriptionGeneration::new(1).unwrap(),
            }
            .canonical_string()
            .unwrap(),
        ),
        target: recipient.clone(),
        mode: None,
        guard: None,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::SubscriptionExpiry {
            scope: "fixture thread".into(),
        },
        body: Some("Renew this subscription".into()),
        activity: None,
        created_at: at(0),
    }
}
fn listing(recipient: &SessionRef, count: u64) -> PushRecordDraft {
    let id = PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap();
    PushRecordDraft {
        push_id: id.clone(),
        kind: PushKind::NotificationListing,
        origin: PushOrigin::Router(PushKind::NotificationListing),
        origin_router_ref: Some(
            RouterOriginRef::NotificationListing {
                target: recipient.clone(),
                listing_id: id,
            }
            .canonical_string()
            .unwrap(),
        ),
        target: recipient.clone(),
        mode: None,
        guard: None,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::NotificationListing { item_count: count },
        body: Some("Notification references and a member-page pointer".into()),
        activity: None,
        created_at: at(600_000),
    }
}
async fn held(store: &InteractionStore, recipient: &SessionRef) -> PushId {
    let record = store.insert_push_record(expiry(recipient)).await.unwrap();
    store.mark_push_attempted(&record.push_id).await.unwrap();
    store
        .hold_push_record(
            &record.push_id,
            receipt(DeliveryOutcome::NotSubmitted {
                reason: "notification window".into(),
                retryable: true,
            }),
        )
        .await
        .unwrap();
    record.push_id
}

#[tokio::test]
async fn startup_attempt_snapshot_never_treats_a_new_attempt_as_interrupted() {
    let directory = tempfile::tempdir().unwrap();
    let store = InteractionStore::open(directory.path().join("interaction.sqlite"))
        .await
        .unwrap();
    let prior_target = target("prior-recipient");
    let fresh_target = target("fresh-recipient");
    let prior = store
        .insert_push_record(expiry(&prior_target))
        .await
        .unwrap();
    store.mark_push_attempted(&prior.push_id).await.unwrap();
    store.close().await.unwrap();
    let store = InteractionStore::open(directory.path().join("interaction.sqlite"))
        .await
        .unwrap();
    let snapshot = store
        .notification_startup_attempt_ids()
        .await
        .unwrap()
        .into_iter()
        .collect();
    let fresh = store
        .insert_push_record(expiry(&fresh_target))
        .await
        .unwrap();
    store.mark_push_attempted(&fresh.push_id).await.unwrap();
    store
        .recover_interrupted_notification_listings(&fresh_target, &snapshot, at(1_000))
        .await
        .unwrap();
    assert!(
        store
            .notification_window(&fresh_target)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .get_push_record(&fresh.push_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Attempted
    );
    store
        .recover_interrupted_notification_listings(&prior_target, &snapshot, at(1_000))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_push_record(&prior.push_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::OutcomeUnknown
    );
    let window = store
        .notification_window(&prior_target)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(window.last_delivered_at, at(1_000));
    store
        .recover_interrupted_notification_listings(&prior_target, &snapshot, at(2_000))
        .await
        .unwrap();
    assert_eq!(
        store
            .notification_window(&prior_target)
            .await
            .unwrap()
            .unwrap(),
        window,
        "terminal-unknown backfill keeps its first settlement"
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn a_known_unsent_carrier_expiry_is_discovered_and_recovered_after_owner_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let recipient = target("expiry-recipient");
    let store = InteractionStore::open(&path).await.unwrap();
    let record = store.insert_push_record(expiry(&recipient)).await.unwrap();
    let held = store
        .hold_unsent_direct_message(
            &record.push_id,
            receipt(DeliveryOutcome::NotSubmitted {
                retryable: true,
                reason: "recipient not loaded".into(),
            }),
        )
        .await
        .unwrap();
    assert_eq!(held.delivery_state, PushDeliveryState::Held);
    assert_eq!(
        store.direct_message_recovery_targets().await.unwrap(),
        vec![recipient.clone()]
    );
    let recovered = store.list_unsent_direct_messages(&recipient).await.unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].push_id, record.push_id);
    store.close().await.unwrap();
    let store = InteractionStore::open(&path).await.unwrap();
    let recovered = store.list_unsent_direct_messages(&recipient).await.unwrap();
    assert_eq!(
        serde_json::to_value(recovered).unwrap(),
        serde_json::to_value(vec![held]).unwrap()
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn a_frozen_listing_survives_reopen_and_pages_every_original_only_for_its_recipient() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let recipient = target("recipient");
    let store = InteractionStore::open(&path).await.unwrap();
    store
        .record_notification_delivery(&recipient, at(0))
        .await
        .unwrap();
    assert_eq!(
        store.hold_notification_window(&recipient).await.unwrap(),
        at(600_000)
    );
    let mut members = Vec::new();
    for _ in 0..55 {
        members.push(held(&store, &recipient).await);
    }
    members.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    let frozen = store
        .freeze_notification_listing(listing(&recipient, 55), &members)
        .await
        .unwrap();
    store.close().await.unwrap();
    let store = InteractionStore::open(&path).await.unwrap();
    let (first, next) = store
        .notification_listing_members(&recipient, &frozen.push_id, None, 50)
        .await
        .unwrap();
    assert_eq!(first.len(), 50);
    assert_eq!(next, Some(members[49].clone()));
    let (rest, end) = store
        .notification_listing_members(&recipient, &frozen.push_id, next.as_ref(), 50)
        .await
        .unwrap();
    assert_eq!(rest.len(), 5);
    assert_eq!(end, None);
    let all = first
        .into_iter()
        .chain(rest)
        .map(|record| record.push_id)
        .collect::<Vec<_>>();
    assert_eq!(all, members);
    assert!(matches!(
        store
            .notification_listing_members(&target("foreign"), &frozen.push_id, None, 50)
            .await,
        Err(StorageError::PushNotPermitted)
    ));
    assert!(matches!(
        store
            .notification_listing_members(&recipient, &frozen.push_id, None, 51)
            .await,
        Err(StorageError::InvalidRecord)
    ));
    store.mark_push_attempted(&frozen.push_id).await.unwrap();
    store
        .settle_notification_listing(
            &frozen.push_id,
            receipt(DeliveryOutcome::Started),
            at(600_000),
        )
        .await
        .unwrap();
    let window = store
        .notification_window(&recipient)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(window.last_delivered_at, at(600_000));
    assert_eq!(window.next_delivery_at().unwrap(), at(1_200_000));
    for id in &members {
        assert_eq!(
            store
                .get_push_record(id)
                .await
                .unwrap()
                .unwrap()
                .delivery_state,
            PushDeliveryState::Delivered
        );
        assert_eq!(
            store
                .notification_listing_for_member(id)
                .await
                .unwrap()
                .unwrap()
                .push_id,
            frozen.push_id
        );
    }
    store.close().await.unwrap();
    let store = InteractionStore::open(&path).await.unwrap();
    assert_eq!(
        store
            .notification_window(&recipient)
            .await
            .unwrap()
            .unwrap(),
        window
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn an_interrupted_listing_is_unknown_and_consumes_a_window_without_a_new_attempt() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let recipient = target("recipient");
    let store = InteractionStore::open(&path).await.unwrap();
    let member = held(&store, &recipient).await;
    let frozen = store
        .freeze_notification_listing(listing(&recipient, 1), std::slice::from_ref(&member))
        .await
        .unwrap();
    store.mark_push_attempted(&frozen.push_id).await.unwrap();
    store.close().await.unwrap();
    let store = InteractionStore::open(&path).await.unwrap();
    store
        .recover_interrupted_notification_listings(
            &recipient,
            &std::collections::HashSet::new(),
            at(700_000),
        )
        .await
        .unwrap();
    let shown = store
        .notification_listing_for_member(&member)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(shown.delivery_state, PushDeliveryState::OutcomeUnknown);
    assert_eq!(
        store
            .get_push_record(&member)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::OutcomeUnknown
    );
    assert!(
        store
            .pending_notification_listings(&recipient)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .notification_window(&recipient)
            .await
            .unwrap()
            .unwrap()
            .next_delivery_at()
            .unwrap(),
        at(1_300_000)
    );
    assert!(matches!(
        store.mark_push_attempted(&frozen.push_id).await,
        Err(StorageError::PushStateConflict)
    ));
    store.close().await.unwrap();
}

#[tokio::test]
async fn known_unsent_releases_members_and_cross_recipient_selection_rolls_back() {
    let directory = tempfile::tempdir().unwrap();
    let store = InteractionStore::open(directory.path().join("interaction.sqlite"))
        .await
        .unwrap();
    let recipient = target("recipient");
    let member = held(&store, &recipient).await;
    let foreign = held(&store, &target("foreign")).await;
    let invalid = listing(&recipient, 2);
    let invalid_id = invalid.push_id.clone();
    assert!(
        store
            .freeze_notification_listing(invalid, &[member.clone(), foreign])
            .await
            .is_err()
    );
    assert!(store.get_push_record(&invalid_id).await.unwrap().is_none());
    assert!(
        store
            .notification_listing_for_member(&member)
            .await
            .unwrap()
            .is_none()
    );
    let frozen = store
        .freeze_notification_listing(listing(&recipient, 1), std::slice::from_ref(&member))
        .await
        .unwrap();
    store.mark_push_attempted(&frozen.push_id).await.unwrap();
    store
        .settle_notification_listing(
            &frozen.push_id,
            receipt(DeliveryOutcome::NotSubmitted {
                reason: "known unsent".into(),
                retryable: true,
            }),
            at(600_000),
        )
        .await
        .unwrap();
    assert!(
        store
            .notification_listing_for_member(&member)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .get_push_record(&member)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Held
    );
    assert!(
        store
            .notification_window(&recipient)
            .await
            .unwrap()
            .is_none()
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn a_corrupt_window_is_refused_by_rust_instead_of_coerced_or_hidden_by_a_sql_constraint() {
    let directory = tempfile::tempdir().unwrap();
    let store = InteractionStore::open(directory.path().join("interaction.sqlite"))
        .await
        .unwrap();
    let recipient = target("recipient");
    store
        .record_notification_delivery(&recipient, at(0))
        .await
        .unwrap();
    sqlx::query("UPDATE notification_windows SET flush_due_at_ms=42")
        .execute(&mut *store.writer.lock().await)
        .await
        .unwrap();
    assert!(matches!(
        store.notification_window(&recipient).await,
        Err(StorageError::InvalidStoredRecord)
    ));
    store.close().await.unwrap();
}

#[tokio::test]
async fn listing_retention_keeps_the_receipt_while_a_recent_terminal_mail_member_still_references_it()
 {
    let directory = tempfile::tempdir().unwrap();
    let store = InteractionStore::open(directory.path().join("interaction.sqlite"))
        .await
        .unwrap();
    let recipient = target("recipient");
    let member = held(&store, &recipient).await;
    let frozen = store
        .freeze_notification_listing(listing(&recipient, 1), std::slice::from_ref(&member))
        .await
        .unwrap();
    store.mark_push_attempted(&frozen.push_id).await.unwrap();
    let settled = at(60 * 24 * 60 * 60 * 1000);
    store
        .settle_notification_listing(&frozen.push_id, receipt(DeliveryOutcome::Started), settled)
        .await
        .unwrap();
    // Arrangement models the SDK projection's terminal-age retention; this test
    // concerns the Router receipt reference, not SDK delivery or native acceptance.
    sqlx::query("UPDATE router_pushes SET retention='mailTerminalAge' WHERE push_id=?")
        .bind(member.as_str())
        .execute(&mut *store.writer.lock().await)
        .await
        .unwrap();
    assert_eq!(
        store
            .prune_push_records(at(61 * 24 * 60 * 60 * 1000), 500)
            .await
            .unwrap(),
        0
    );
    assert!(
        store
            .notification_listing_for_member(&member)
            .await
            .unwrap()
            .is_some()
    );
    store.close().await.unwrap();
}
