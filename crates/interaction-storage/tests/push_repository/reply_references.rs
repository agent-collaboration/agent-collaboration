//! A reply names a global item; local history is not its existence authority.
use super::*;
use interaction_storage::{DirectMessageHistoryQuery, PushInboxQuery};

#[tokio::test]
async fn a_missing_parent_is_normal_for_get_inbox_pending_and_history_readers() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("native store");
    let sender = session("claude-local", "remote-reply-sender");
    let target = session("codex-local", "remote-reply-recipient");
    let parent: PushId = "01900000-0000-7000-8000-000000000730"
        .to_owned()
        .try_into()
        .unwrap();
    let child = push_id();
    let mut draft = push_draft(
        child.clone(),
        target.clone(),
        PushOrigin::Session(sender.clone()),
        "A reply whose original has no local history row",
        Utc::now(),
    );
    draft.reply_to_push_id = Some(parent.clone());
    store
        .insert_push_record(draft)
        .await
        .expect("global reply reference");
    store.close().await.expect("close every native handle");
    let store = InteractionStore::open(&database.path)
        .await
        .expect("owning reopen");
    assert!(store.get_push_record(&parent).await.unwrap().is_none());
    assert_eq!(
        store
            .get_push_record(&child)
            .await
            .unwrap()
            .unwrap()
            .reply_to_push_id,
        Some(parent.clone())
    );
    let pending = store.list_pending_push_records(&target).await.unwrap();
    let inbox = store
        .list_direct_message_inbox(&PushInboxQuery {
            target: target.clone(),
            limit: 10,
        })
        .await
        .unwrap();
    let history = store
        .list_direct_message_history(&DirectMessageHistoryQuery {
            caller: target,
            with: sender,
            limit: 10,
        })
        .await
        .unwrap();
    for records in [pending, inbox, history] {
        assert_eq!(records.len(), 1, "missing parent hid the retained reply");
        assert_eq!(records[0].push_id, child);
        assert_eq!(records[0].reply_to_push_id, Some(parent.clone()));
    }
    store.close().await.unwrap();
}

#[tokio::test]
async fn a_local_reply_keeps_its_reference_after_parent_pruning_and_reopen() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("native store");
    let sender = session("claude-local", "local-original-sender");
    let recipient = session("codex-local", "local-original-recipient");
    let now: chrono::DateTime<Utc> = "2026-10-20T12:00:00Z".parse().unwrap();
    let parent = push_id();
    store
        .insert_push_record(push_draft(
            parent.clone(),
            recipient.clone(),
            PushOrigin::Session(sender.clone()),
            "Original local message",
            now - Duration::days(31),
        ))
        .await
        .unwrap();
    let child = push_id();
    let mut reply = push_draft(
        child.clone(),
        sender.clone(),
        PushOrigin::Session(recipient.clone()),
        "Fresh local reply",
        now,
    );
    reply.reply_to_push_id = Some(parent.clone());
    store.insert_push_record(reply).await.unwrap();
    assert_eq!(
        store
            .get_push_record(&child)
            .await
            .unwrap()
            .unwrap()
            .reply_to_push_id,
        Some(parent.clone()),
        "local reply did not link before pruning"
    );
    assert_eq!(store.prune_push_records(now, 500).await.unwrap(), 1);
    store.close().await.unwrap();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("owning reopen");
    assert!(store.get_push_record(&parent).await.unwrap().is_none());
    let history = store
        .list_direct_message_history(&DirectMessageHistoryQuery {
            caller: sender,
            with: recipient,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].push_id, child);
    assert_eq!(
        history[0].reply_to_push_id,
        Some(parent),
        "pruning local history erased the global reply reference"
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn a_malformed_stored_reply_reference_is_refused_instead_of_becoming_absent() {
    let database = TestDatabase::new();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("native store");
    let target = session("codex-local", "corrupt-reference-recipient");
    let child = push_id();
    store
        .insert_push_record(push_draft(
            child.clone(),
            target.clone(),
            PushOrigin::Session(session("claude-local", "corrupt-reference-sender")),
            "Retained reply",
            Utc::now(),
        ))
        .await
        .unwrap();
    store.close().await.unwrap();
    let mut raw = TursoConnection::connect_with(
        &TursoConnectOptions::new()
            .filename(&database.path)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE router_pushes SET reply_to_push_id=? WHERE push_id=?")
        .bind("not-a-canonical-item")
        .bind(child.as_str())
        .execute(&mut raw)
        .await
        .unwrap();
    raw.close().await.unwrap();
    let store = InteractionStore::open(&database.path)
        .await
        .expect("valid owned schema");
    assert!(matches!(
        store.get_push_record(&child).await,
        Err(StorageError::InvalidRecord)
    ));
    assert!(matches!(
        store.list_pending_push_records(&target).await,
        Err(StorageError::InvalidRecord)
    ));
    store.close().await.unwrap();
}
