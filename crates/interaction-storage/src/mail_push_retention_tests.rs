//! Mail projection retention remains valid independently of the SDK custody row.
use super::*;
use crate::{MailPushUpdate, StorageError};
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt};

#[tokio::test]
async fn mail_projection_retention_survives_reopen_without_custody_and_rejects_a_corrupt_class() {
    // Projection-only input: no SDK row is inserted or deleted by this storage fixture.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let (_, mut draft) = native_item_and_push();
    let created = chrono::Utc::now() - chrono::Duration::days(80);
    draft.created_at = created;
    let id = draft.push_id.clone();
    let pending_id = id.clone();
    store
        .transaction(move |scope| {
            Box::pin(async move {
                crate::insert_mail_push_in_scope(scope, draft)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await
        .unwrap();
    assert_eq!(
        owner
            .prune_push_records(created + chrono::Duration::days(39), 500)
            .await
            .unwrap(),
        0
    );
    store
        .transaction(move |scope| {
            Box::pin(async move {
                crate::mark_mail_push_attempted_in_scope(scope, &pending_id)
                    .await
                    .map_err(StoreError::backend)?;
                crate::apply_mail_push_outcome_in_scope(
                    scope,
                    &pending_id,
                    MailPushUpdate::Finished {
                        receipt: DeliveryReceipt {
                            outcome: DeliveryOutcome::PeerMessageWritten,
                            reachability: None,
                            client: None,
                        },
                        settled_at: created + chrono::Duration::days(40),
                    },
                )
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await
        .unwrap();
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("retained owner"))
        .close()
        .await
        .unwrap();
    let owner = InteractionStore::open(&path).await.unwrap();
    assert_eq!(
        owner
            .prune_push_records(created + chrono::Duration::days(70), 500)
            .await
            .unwrap(),
        0
    );
    assert!(owner.get_push_record(&id).await.unwrap().is_some());
    let mut connection = owner.writer.lock().await;
    sqlx::query::<sqlx_turso::Turso>(
        "UPDATE router_pushes SET retention='badRetention' WHERE push_id=?",
    )
    .bind(id.as_str())
    .execute(&mut *connection)
    .await
    .unwrap();
    drop(connection);
    assert!(matches!(
        owner.get_push_record(&id).await,
        Err(StorageError::InvalidRecord)
    ));
    assert!(matches!(
        owner
            .prune_push_records(created + chrono::Duration::days(71), 500)
            .await,
        Err(StorageError::InvalidRecord)
    ));
    let mut connection = owner.writer.lock().await;
    sqlx::query::<sqlx_turso::Turso>(
        "UPDATE router_pushes SET retention='mailTerminalAge' WHERE push_id=?",
    )
    .bind(id.as_str())
    .execute(&mut *connection)
    .await
    .unwrap();
    drop(connection);
    assert_eq!(
        owner
            .prune_push_records(created + chrono::Duration::days(71), 500)
            .await
            .unwrap(),
        1
    );
    owner.close().await.unwrap();
}

#[tokio::test]
async fn nonterminal_mail_projection_clock_is_refused_before_read_prune_or_carrier_attempt() {
    let directory = tempfile::tempdir().unwrap();
    let owner = Arc::new(
        InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .unwrap(),
    );
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let (_, mut draft) = native_item_and_push();
    let created = chrono::Utc::now() - chrono::Duration::days(80);
    draft.created_at = created;
    let id = draft.push_id.clone();
    store
        .transaction(move |scope| {
            Box::pin(async move {
                crate::insert_mail_push_in_scope(scope, draft)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await
        .unwrap();
    let receipt = serde_json::json!({"outcome":{"kind":"rejected","reason":"noRoute","nextAction":"retryLater"},"reachability":null,"client":null}).to_string();
    let mut connection = owner.writer.lock().await;
    sqlx::query::<sqlx_turso::Turso>(
        "UPDATE router_pushes SET last_outcome_json=?,settled_at=? WHERE push_id=?",
    )
    .bind(&receipt)
    .bind((created + chrono::Duration::days(1)).to_rfc3339())
    .bind(id.as_str())
    .execute(&mut *connection)
    .await
    .unwrap();
    drop(connection);
    assert!(owner.get_push_record(&id).await.is_err());
    assert!(
        owner
            .prune_push_records(created + chrono::Duration::days(40), 500)
            .await
            .is_err()
    );
    assert!(owner.mark_push_attempted(&id).await.is_err());
    let mut connection = owner.writer.lock().await;
    let kept: i64 = sqlx::query_scalar::<sqlx_turso::Turso, i64>("SELECT COUNT(*) FROM router_pushes WHERE push_id=? AND delivery_state='pending' AND last_outcome_json=? AND settled_at IS NOT NULL")
        .bind(id.as_str()).bind(receipt).fetch_one(&mut *connection).await.unwrap();
    assert_eq!(kept, 1);
    drop(connection);
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("retained owner"))
        .close()
        .await
        .unwrap();
}
