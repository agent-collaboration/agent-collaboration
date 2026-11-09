use super::*;
use crate::StorageError;
use chrono::{DateTime, Utc};
use collaboration_protocol::StuckMailSummary;

#[tokio::test]
async fn sent_age_counts_only_custody_older_than_five_minutes_and_uses_oldest_handoff() {
    let directory = tempfile::tempdir().unwrap();
    let owner = Arc::new(
        InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .unwrap(),
    );
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let now = DateTime::<Utc>::from_timestamp_millis(1_000_000).unwrap();
    assert_eq!(
        owner.stuck_sent_mail(now).await.unwrap(),
        StuckMailSummary::Empty
    );
    for (status, age_seconds) in [
        ("sent", 601),
        ("sent", 301),
        ("sent", 300),
        ("sent", 20),
        ("held", 900),
    ] {
        let (item, draft) = native_item_and_push();
        let id = item.id().as_uuid().to_string();
        enqueue_and_project(&store, item, draft).await.unwrap();
        // Real SDK admission supplies the full valid custody row; this test
        // sets historical status/time inputs to exercise only the read view.
        sqlx::query(
            "UPDATE _fleet_sync_outbox_items SET status_kind=?, handed_off_at=? WHERE item_id=?",
        )
        .bind(status)
        .bind(now.timestamp_millis() - age_seconds * 1000)
        .bind(id)
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    }
    assert_eq!(
        owner.stuck_sent_mail(now).await.unwrap(),
        StuckMailSummary::Stuck {
            count: std::num::NonZeroU64::new(2).unwrap(),
            oldest_age_seconds: 601
        }
    );
    sqlx::query(
        "UPDATE _fleet_sync_outbox_items SET status_kind='futureStatus' WHERE status_kind='held'",
    )
    .execute(&mut *owner.writer.lock().await)
    .await
    .unwrap();
    assert!(owner.stuck_sent_mail(now).await.is_err());
}

#[tokio::test]
async fn valid_sdk_recovered_sent_without_handoff_is_outstanding_with_unknown_age() {
    use fleet_sync_delivery::{OutboxStatusState, SubmitInput};
    use fleet_sync_protocol::{DeliveryStatus, StatusVersion, SubmitResult};
    let directory = tempfile::tempdir().unwrap();
    let owner = Arc::new(
        InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .unwrap(),
    );
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let (item, _draft) = native_item_and_push();
    let item_id = item.id();
    let input = SubmitInput {
        item: item_id,
        digest: item.digest().clone(),
        relay: PeerId::new(uuid::Uuid::now_v7()).unwrap(),
        mailbox: item.recipient().peer,
    };
    let now = Utc::now();
    store
        .transaction(move |scope| {
            Box::pin(async move {
                let limits = DeliveryLimits::new::<BoardDeliveryTypes>(
                    16 * 1024 * 1024,
                    20,
                    16 * 1024 * 1024,
                    minimum_settlement_allowance::<BoardDeliveryTypes>()
                        .map_err(StoreError::backend)?,
                )
                .map_err(StoreError::backend)?;
                let mut outbox =
                    Outbox::<_, BoardDeliveryTypes, _>::new(scope, limits, &NoDeliveryConsumer);
                // No prior header: the real public SDK path calls save_recovered.
                outbox
                    .mark_submitted(
                        &input,
                        SubmitResult::Accepted {
                            status: DeliveryStatus::Sent,
                            version: StatusVersion::new(1),
                            cancel: None,
                        },
                        SyncTime::from_unix_millis(now.timestamp_millis()),
                    )
                    .await
                    .map_err(StoreError::backend)?;
                assert!(matches!(
                    outbox
                        .status(item_id)
                        .await
                        .map_err(StoreError::backend)?
                        .state,
                    OutboxStatusState::Known {
                        status: DeliveryStatus::Sent,
                        ..
                    }
                ));
                Ok(())
            })
        })
        .await
        .unwrap();
    let row: (String, String, i64, Option<i64>) = sqlx::query_as(
        "SELECT record_kind, status_kind, live, handed_off_at FROM _fleet_sync_outbox_items WHERE item_id=?")
        .bind(item_id.as_uuid().to_string()).fetch_one(&mut *owner.writer.lock().await).await.unwrap();
    assert_eq!(row, ("recovered".to_owned(), "sent".to_owned(), 0, None));
    assert_eq!(
        owner.stuck_sent_mail(now).await.unwrap(),
        StuckMailSummary::AgeUnknown {
            recovered_count: std::num::NonZeroU64::MIN,
            known_stuck: None,
        }
    );
    let (known, draft) = native_item_and_push();
    let known_id = known.id().as_uuid().to_string();
    enqueue_and_project(&store, known, draft).await.unwrap();
    sqlx::query(
        "UPDATE _fleet_sync_outbox_items SET status_kind='sent', handed_off_at=? WHERE item_id=?",
    )
    .bind(now.timestamp_millis() - 601_000)
    .bind(known_id)
    .execute(&mut *owner.writer.lock().await)
    .await
    .unwrap();
    assert_eq!(
        owner.stuck_sent_mail(now).await.unwrap(),
        StuckMailSummary::AgeUnknown {
            recovered_count: std::num::NonZeroU64::MIN,
            known_stuck: Some(collaboration_protocol::KnownStuckMail {
                count: std::num::NonZeroU64::MIN,
                oldest_age_seconds: 601,
            }),
        }
    );
    sqlx::query("UPDATE _fleet_sync_outbox_items SET handed_off_at=X'01' WHERE item_id=?")
        .bind(item_id.as_uuid().to_string())
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    assert!(matches!(
        owner.stuck_sent_mail(now).await,
        Err(StorageError::InvalidStoredRecord)
    ));
    sqlx::query("UPDATE _fleet_sync_outbox_items SET record_kind='unresolvable', handed_off_at=NULL WHERE item_id=?")
        .bind(item_id.as_uuid().to_string()).execute(&mut *owner.writer.lock().await).await.unwrap();
    assert!(matches!(
        owner.stuck_sent_mail(now).await,
        Err(StorageError::InvalidStoredRecord)
    ));
}
