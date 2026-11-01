//! Private SDK outcomes are lossless; malformed stored attribution never becomes absence.
use super::*;
use crate::{
    LocalCancelPreparation, StorageError, prepare_local_cancel_in_scope,
    record_local_cancel_outcome_in_scope,
};
use collaboration_mail_protocol::{CancelCause, CarrierAcceptance, MailItemId};
use fleet_sync_protocol::RecipientOutcome;

fn first_clock() -> chrono::DateTime<chrono::Utc> {
    "2026-10-20T12:00:00Z".parse().unwrap()
}

#[tokio::test]
async fn private_sdk_failure_roundtrips_without_changing_first_attribution_or_clock() {
    // Arrange: this failure is produced by the SDK but has no public board equivalent.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let item = MailItemId::generate();
    let target = MailItemId::generate();
    let original = CancelCause::ByRequester {
        by: serde_json::from_value(json!({"kind":"human","humanId":"cancel-requester"})).unwrap(),
    };
    let failure = RecipientOutcome::<CarrierAcceptance>::Refused {
        reason: DeliveryRefusal::ProcessingFailed {
            kind: DeliveryFailureKind::CustodyCorrupt,
        },
    };
    let cancel_id = item.clone();
    let target_id = target.clone();
    let expected_attribution = original.clone();
    let expected_outcome = failure.clone();
    // Act: duplicate preparation must keep the first requester and first clock.
    store
        .transaction(move |scope| {
            Box::pin(async move {
                assert_eq!(
                    prepare_local_cancel_in_scope(
                        scope,
                        &cancel_id,
                        &target_id,
                        &original,
                        first_clock()
                    )
                    .await
                    .map_err(StoreError::backend)?,
                    LocalCancelPreparation::Inserted
                );
                assert_eq!(
                    prepare_local_cancel_in_scope(
                        scope,
                        &cancel_id,
                        &target_id,
                        &CancelCause::BoardDerived {},
                        first_clock() + chrono::Duration::days(1)
                    )
                    .await
                    .map_err(StoreError::backend)?,
                    LocalCancelPreparation::Kept
                );
                record_local_cancel_outcome_in_scope(
                    scope,
                    &cancel_id,
                    &target_id,
                    &failure,
                    first_clock().timestamp_millis(),
                )
                .await
                .map_err(StoreError::backend)
            })
        })
        .await
        .unwrap();
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("owner retained"))
        .close()
        .await
        .unwrap();
    let owner = InteractionStore::open(&path).await.unwrap();
    // Assert after cold reopen, through the owning typed reader.
    let stored = owner.local_cancel_delivery(&item).await.unwrap().unwrap();
    assert_eq!(stored.item_id, item);
    assert_eq!(stored.target_item, target);
    assert_eq!(stored.attribution, expected_attribution);
    assert_eq!(stored.outcome, Some(expected_outcome));
    assert_eq!(stored.recorded_at, first_clock());
    let settled_at_ms = stored.settled_at.unwrap().timestamp_millis();
    sqlx::query("UPDATE local_cancel_deliveries SET settled_at_ms=NULL WHERE item_id=?")
        .bind(item.as_str())
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    assert!(matches!(
        owner.local_cancel_delivery(&item).await,
        Err(StorageError::InvalidStoredRecord)
    ));
    sqlx::query("UPDATE local_cancel_deliveries SET settled_at_ms=? WHERE item_id=?")
        .bind(settled_at_ms)
        .bind(item.as_str())
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    owner.close().await.unwrap();
}

#[tokio::test]
async fn invalid_private_cancel_rows_refuse_on_cold_read() {
    let item = MailItemId::generate();
    let other_cancel = MailItemId::generate();
    let invalid = [
        ("target_item", "not-a-uuid".to_owned()),
        ("attribution_json", "{\"kind\":\"futureCause\"}".to_owned()),
        ("attribution_json", "{\"kind\":\"bySender\",\"unknown\":true}".to_owned()),
        ("outcome_json", "{".to_owned()),
        ("outcome_json", "{\"kind\":\"futureOutcome\"}".to_owned()),
        ("outcome_json", "{\"kind\":\"refused\",\"value\":{\"reason\":{\"kind\":\"processingFailed\",\"value\":{\"kind\":\"futureFailure\"}}}}".to_owned()),
        ("outcome_json", format!("{{\"kind\":\"prevented\",\"value\":{{\"cancel\":\"{}\"}}}}", other_cancel.as_str())),
        ("recorded_at", "not-a-clock".to_owned()),
        ("settled_at_ms", "1".to_owned()),
    ];
    for (column, bad_value) in invalid {
        // Arrange: a structurally owned schema with one malformed row, not a foreign file.
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("interaction.sqlite");
        let owner = InteractionStore::open(&path).await.unwrap();
        let target = MailItemId::generate();
        sqlx::query("INSERT INTO local_cancel_deliveries(item_id,target_item,attribution_json,outcome_json,recorded_at) VALUES (?,?,'{\"kind\":\"bySender\"}',NULL,'2026-10-20T12:00:00.000000000Z')")
            .bind(item.as_str()).bind(target.as_str())
            .execute(&mut *owner.writer.lock().await).await.unwrap();
        let corrupt_row = match column {
            "target_item" => "UPDATE local_cancel_deliveries SET target_item=? WHERE item_id=?",
            "attribution_json" => {
                "UPDATE local_cancel_deliveries SET attribution_json=? WHERE item_id=?"
            }
            "outcome_json" => "UPDATE local_cancel_deliveries SET outcome_json=? WHERE item_id=?",
            "recorded_at" => "UPDATE local_cancel_deliveries SET recorded_at=? WHERE item_id=?",
            "settled_at_ms" => {
                "UPDATE local_cancel_deliveries SET settled_at_ms=CAST(? AS INTEGER) WHERE item_id=?"
            }
            _ => panic!("unlisted corruption column"),
        };
        sqlx::query(corrupt_row)
            .bind(bad_value)
            .bind(item.as_str())
            .execute(&mut *owner.writer.lock().await)
            .await
            .unwrap();
        owner.close().await.unwrap();
        // Act / assert: malformed metadata must be reported, never silently treated as no record.
        let owner = InteractionStore::open(&path).await.unwrap();
        assert!(
            matches!(
                owner.local_cancel_delivery(&item).await,
                Err(StorageError::InvalidStoredRecord)
            ),
            "column={column}"
        );
        owner.close().await.unwrap();
    }
}
