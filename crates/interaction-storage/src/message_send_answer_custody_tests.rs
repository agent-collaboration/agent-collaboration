//! Real sender custody and its replay answer share the owning transaction.
use super::*;
use collaboration_protocol::{MailItemId, MessageSendResult, QueuedMessageSend};

async fn enqueue_with_answer(
    adapter: &NativeStore,
    item: DeliveryItem<BoardDeliveryTypes>,
    target: collaboration_protocol::SessionRef,
) -> Result<(), StoreError> {
    adapter
        .transaction(move |scope| {
            Box::pin(async move {
                let item_id: MailItemId = item.id().as_uuid().to_string().try_into().unwrap();
                // Quotas and relay are fixture configuration; custody and the RPC projection are real.
                let limits = DeliveryLimits::new::<BoardDeliveryTypes>(
                    16 * 1024 * 1024,
                    20,
                    16 * 1024 * 1024,
                    20,
                    16 * 1024 * 1024,
                    minimum_settlement_allowance::<BoardDeliveryTypes>()
                        .map_err(StoreError::backend)?,
                )
                .map_err(StoreError::backend)?;
                let (admission, operation) = Outbox::<_, BoardDeliveryTypes>::new(scope, limits)
                    .enqueue_and_admit::<NativeSubmit>(
                        item,
                        PeerId::new(uuid::Uuid::now_v7()).unwrap(),
                    )
                    .await
                    .map_err(StoreError::backend)?;
                assert!(matches!(admission, OutboxAdmission::Queued { .. }));
                assert!(operation.is_some());
                crate::record_message_send_admission(
                    scope,
                    &item_id,
                    "remote recipient",
                    chrono::Utc::now(),
                )
                .await
                .map_err(StoreError::backend)?;
                crate::record_message_send_outcome(
                    scope,
                    &item_id,
                    &MessageSendResult::Queued {
                        queued: QueuedMessageSend {
                            item: item_id.clone(),
                            target,
                            target_identity: "remote recipient".into(),
                        },
                    },
                    chrono::Utc::now().timestamp_millis(),
                )
                .await
                .map_err(|error| StoreError::backend(format!("queued answer write: {error:?}")))?;
                Ok(())
            })
        })
        .await
}

#[tokio::test]
async fn queued_answer_reopens_with_custody_and_failure_rolls_back_without_a_push() {
    // Arrange: the same physical owner contains library custody, journal and answer projection.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    let adapter = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let (committed_item, draft) = native_item_and_push();
    let id: MailItemId = committed_item
        .id()
        .as_uuid()
        .to_string()
        .try_into()
        .unwrap();
    let expected =
        json!({"queued":{"item":id,"target":draft.target,"targetIdentity":"remote recipient"}});
    enqueue_with_answer(&adapter, committed_item, draft.target.clone())
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TRIGGER reject_queued_answer BEFORE UPDATE ON message_send_answers BEGIN SELECT RAISE(ABORT, 'fixture queued answer rejection'); END")
        .execute(&mut *owner.writer.lock().await).await.unwrap();

    // Act: fail after actual outbox and journal writes, then cold reopen the file owner.
    let (rejected_item, rejected_draft) = native_item_and_push();
    let rejected_id: MailItemId = rejected_item.id().as_uuid().to_string().try_into().unwrap();
    let error = enqueue_with_answer(&adapter, rejected_item, rejected_draft.target)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("fixture queued answer rejection"),
        "{error:?}"
    );
    sqlx::raw_sql("DROP TRIGGER reject_queued_answer")
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    drop(adapter);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("retained owner"))
        .close()
        .await
        .unwrap();
    let owner = InteractionStore::open(&path).await.unwrap();

    // Assert: no push was invented, and the failed projection left no custody or journal row.
    let answer = owner.message_send_answer(&id).await.unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(answer.result.unwrap()).unwrap(),
        expected
    );
    let settled_at_ms = answer.settled_at.unwrap().timestamp_millis();
    assert!(
        owner
            .message_send_answer(&rejected_id)
            .await
            .unwrap()
            .is_none()
    );
    let counts: (i64,i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM router_pushes),(SELECT count(*) FROM _fleet_sync_outbox_items),(SELECT count(*) FROM _fleet_sync_operations),(SELECT count(*) FROM message_send_answers)")
        .fetch_one(&mut *owner.writer.lock().await).await.unwrap();
    assert_eq!(counts, (0, 1, 1, 1));

    sqlx::query("UPDATE message_send_answers SET settled_at_ms=NULL WHERE item_id=?")
        .bind(id.as_str())
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    assert!(matches!(
        owner.message_send_answer(&id).await,
        Err(crate::StorageError::InvalidStoredRecord)
    ));
    sqlx::query("UPDATE message_send_answers SET settled_at_ms=? WHERE item_id=?")
        .bind(settled_at_ms)
        .bind(id.as_str())
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();

    // Corrupt a saved identity: decode must refuse rather than replay a different item.
    let mut corrupt = expected;
    corrupt["queued"]["item"] = json!(rejected_id);
    sqlx::query("UPDATE message_send_answers SET result=? WHERE item_id=?")
        .bind(corrupt.to_string())
        .bind(id.as_str())
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    assert!(matches!(
        owner.message_send_answer(&id).await,
        Err(crate::StorageError::InvalidStoredRecord)
    ));
    sqlx::query("INSERT INTO message_send_answers(item_id,target_identity,result,recorded_at,settled_at_ms) VALUES (?,?,NULL,'2026-10-20T12:00:00.000000000Z',1)")
        .bind(rejected_id.as_str())
        .bind("pending recipient")
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    assert!(matches!(
        owner.message_send_answer(&rejected_id).await,
        Err(crate::StorageError::InvalidStoredRecord)
    ));
    owner.close().await.unwrap();
}
