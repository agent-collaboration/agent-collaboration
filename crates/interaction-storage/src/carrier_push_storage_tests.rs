//! Native carrier recovery and transitions never acquire SDK projection rows.
use super::*;
use crate::StorageError;
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, PushDeliveryState};

fn held_receipt() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::NotSubmitted {
            retryable: true,
            reason: "fixture target is absent".into(),
        },
        reachability: None,
        client: None,
    }
}

#[tokio::test]
async fn carrier_recovery_and_transitions_leave_sdk_rows_to_their_scoped_owner_after_reopen() {
    // Arrange real native projections in both classes. SDK custody is outside this storage case.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    let (_, copied) = native_item_and_push();
    let copied_id = copied.push_id.clone();
    let copied_target = copied.target.clone();
    owner.insert_push_record(copied).await.unwrap();
    owner
        .hold_unsent_direct_message(&copied_id, held_receipt())
        .await
        .unwrap();
    let (_, mut sdk) = native_item_and_push();
    sdk.target.session_id = "sdk-only-reader".to_owned().try_into().unwrap();
    let sdk_id = sdk.push_id.clone();
    let sdk_target = sdk.target.clone();
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    store
        .transaction(move |scope| {
            Box::pin(async move {
                crate::insert_mail_push_in_scope(scope, sdk)
                    .await
                    .map_err(StoreError::backend)?;
                crate::apply_mail_push_outcome_in_scope(
                    scope,
                    &sdk_id,
                    crate::MailPushUpdate::Held {
                        receipt: held_receipt(),
                        held_at: chrono::Utc::now(),
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
        .unwrap_or_else(|_| panic!("owner retained"))
        .close()
        .await
        .unwrap();

    // Act through the same recovery queries the production carrier consumes after restart.
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    assert_eq!(
        owner.direct_message_recovery_targets().await.unwrap(),
        vec![copied_target.clone()]
    );
    assert_eq!(
        owner
            .list_unsent_direct_messages(&copied_target)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.push_id)
            .collect::<Vec<_>>(),
        vec![copied_id.clone()]
    );
    assert!(
        owner
            .list_unsent_direct_messages(&sdk_target)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        owner
            .list_held_carrier_push_records(&sdk_target)
            .await
            .unwrap()
            .is_empty()
    );
    let sdk_id = owner
        .list_held_push_records(&sdk_target)
        .await
        .unwrap()
        .remove(0)
        .push_id;
    assert!(matches!(
        owner.mark_push_attempted(&sdk_id).await,
        Err(StorageError::PushStateConflict)
    ));
    assert!(matches!(
        owner
            .hold_unsent_direct_message(&sdk_id, held_receipt())
            .await,
        Err(StorageError::PushStateConflict)
    ));
    owner.mark_push_attempted(&copied_id).await.unwrap();
    assert_eq!(
        owner
            .settle_interrupted_pushes(chrono::Utc::now())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        owner
            .get_push_record(&copied_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::OutcomeUnknown
    );
    assert_eq!(
        owner
            .get_push_record(&sdk_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Held
    );

    // An interrupted SDK projection remains for SDK recovery, never carrier Unknown settlement.
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    store
        .transaction(move |scope| {
            Box::pin(async move {
                crate::mark_mail_push_attempted_in_scope(scope, &sdk_id)
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(
        owner
            .settle_interrupted_pushes(chrono::Utc::now())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        owner
            .list_pending_push_records(&sdk_target)
            .await
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        owner
            .get_push_record(&copied_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::OutcomeUnknown
    );
    let mut connection = owner.writer.lock().await;
    let state: String = sqlx::query_scalar::<sqlx_turso::Turso, String>(
        "SELECT delivery_state FROM router_pushes WHERE retention='mailTerminalAge'",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(state, "attempted");
    drop(connection);
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("owner retained"))
        .close()
        .await
        .unwrap();
}
