//! Owned observation rows fail closed independently of an allocation scan.
use super::*;
use crate::{MailPushUpdate, StorageError};
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt};
use serde_json::json;

#[tokio::test]
async fn provider_route_observations_validate_the_target_and_clear_with_the_next_observation() {
    // Native projection metadata only; this case does not claim SDK custody or a client call.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    let (_, draft) = native_item_and_push();
    let push = draft.push_id.clone();
    let key = push.clone();
    let route: collaboration_protocol::ProviderAcpRouteEvidence = serde_json::from_value(json!({
        "target": draft.target,
        "generation": { "serviceEpoch": "00000000-0000-4000-8000-000000000001", "generation": 1 },
        "binding": "stored-queued-provider",
        "attemptId": uuid::Uuid::now_v7().to_string(),
        "submission": "routerQueued", "settlement": "notObserved"
    }))
    .unwrap();
    let original = serde_json::to_value(&route).unwrap();
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let for_write = route.clone();
    store
        .transaction(move |scope| {
            Box::pin(async move {
                crate::insert_mail_push_in_scope(scope, draft)
                    .await
                    .map_err(StoreError::backend)?;
                let record = crate::apply_mail_push_outcome_in_scope(
                    scope,
                    &key,
                    MailPushUpdate::Finished {
                        receipt: DeliveryReceipt {
                            outcome: DeliveryOutcome::Unknown,
                            reachability: None,
                            client: None,
                        },
                        settled_at: chrono::Utc::now(),
                    },
                )
                .await
                .map_err(StoreError::backend)?
                .unwrap();
                crate::record_mail_route_evidence_in_scope(scope, &record, Some(&for_write))
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(
            owner
                .mail_delivery_observation(&push)
                .await
                .unwrap()
                .unwrap()
                .route_evidence
                .unwrap()
        )
        .unwrap(),
        original
    );

    let mut wrong_target = original.clone();
    wrong_target["target"]["sessionId"] = json!("a-different-native-reader");
    let mut invalid_settlement = original.clone();
    invalid_settlement["settlement"] = json!("confirmed");
    let mut unknown_field = original.clone();
    unknown_field["unownedField"] = json!(true);
    for corrupt in [
        json!({"invalid":true}),
        wrong_target,
        invalid_settlement,
        unknown_field,
    ] {
        let mut writer = owner.writer.lock().await;
        sqlx::query("UPDATE router_pushes SET last_route_evidence_json=? WHERE push_id=?")
            .bind(serde_json::to_string(&corrupt).unwrap())
            .bind(push.as_str())
            .execute(&mut *writer)
            .await
            .unwrap();
        drop(writer);
        assert!(
            matches!(
                owner.mail_delivery_observation(&push).await,
                Err(StorageError::InvalidStoredRecord)
            ),
            "{corrupt}"
        );
        let mut writer = owner.writer.lock().await;
        sqlx::query("UPDATE router_pushes SET last_route_evidence_json=? WHERE push_id=?")
            .bind(serde_json::to_string(&original).unwrap())
            .bind(push.as_str())
            .execute(&mut *writer)
            .await
            .unwrap();
    }
    // Incoming mismatched context rolls back through the same borrowed native transaction.
    let key = push.clone();
    let mut invalid = route;
    invalid.target.session_id = "another-reader".to_owned().try_into().unwrap();
    assert!(
        store
            .transaction(move |scope| Box::pin(async move {
                let record = crate::read_mail_push_in_scope(scope, &key)
                    .await
                    .map_err(StoreError::backend)?
                    .unwrap();
                crate::record_mail_route_evidence_in_scope(scope, &record, Some(&invalid))
                    .await
                    .map_err(StoreError::backend)
            }))
            .await
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(
            owner
                .mail_delivery_observation(&push)
                .await
                .unwrap()
                .unwrap()
                .route_evidence
                .unwrap()
        )
        .unwrap(),
        original
    );
    let key = push.clone();
    store
        .transaction(move |scope| {
            Box::pin(async move {
                let record = crate::read_mail_push_in_scope(scope, &key)
                    .await
                    .map_err(StoreError::backend)?
                    .unwrap();
                crate::record_mail_route_evidence_in_scope(scope, &record, None)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await
        .unwrap();
    assert!(
        owner
            .mail_delivery_observation(&push)
            .await
            .unwrap()
            .unwrap()
            .route_evidence
            .is_none()
    );
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("observation owner retained"))
        .close()
        .await
        .unwrap();
    let owner = InteractionStore::open(&path).await.unwrap();
    assert!(
        owner
            .mail_delivery_observation(&push)
            .await
            .unwrap()
            .unwrap()
            .route_evidence
            .is_none()
    );
    owner.close().await.unwrap();
}

#[tokio::test]
async fn invalid_stored_native_observations_refuse_without_coercion() {
    let directory = tempfile::tempdir().unwrap();
    let owner = Arc::new(
        InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .unwrap(),
    );
    let (_, draft) = native_item_and_push();
    let push = draft.push_id.clone();
    let key = push.clone();
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    store
        .transaction(move |scope| {
            Box::pin(async move {
                crate::insert_mail_push_in_scope(scope, draft)
                    .await
                    .map_err(StoreError::backend)?;
                crate::apply_mail_push_outcome_in_scope(
                    scope,
                    &key,
                    MailPushUpdate::Held {
                        receipt: DeliveryReceipt {
                            outcome: DeliveryOutcome::NotSubmitted {
                                retryable: true,
                                reason: "reader absent".into(),
                            },
                            reachability: None,
                            client: None,
                        },
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
    assert!(
        owner
            .mail_delivery_observation(&push)
            .await
            .unwrap()
            .is_some()
    );
    for statement in [
        "UPDATE router_pushes SET last_outcome_at='invalid-time'",
        "UPDATE router_pushes SET last_outcome_json='{\"outcome\":{\"kind\":\"started\"},\"reachability\":null,\"client\":null}'",
        "UPDATE router_pushes SET delivery_state='unsupported-state'",
    ] {
        let original = owner
            .mail_delivery_observation(&push)
            .await
            .unwrap()
            .unwrap();
        let mut writer = owner.writer.lock().await;
        sqlx::query(statement).execute(&mut *writer).await.unwrap();
        drop(writer);
        assert!(matches!(
            owner.mail_delivery_observation(&push).await,
            Err(StorageError::InvalidStoredRecord | StorageError::InvalidRecord)
        ));
        let mut writer = owner.writer.lock().await;
        sqlx::query(
            "UPDATE router_pushes SET delivery_state='held',last_outcome_json=?,last_outcome_at=?",
        )
        .bind(serde_json::to_string(&original.last_outcome.unwrap()).unwrap())
        .bind(crate::push_record_rows::serialize_timestamp(
            original.recorded_at.unwrap(),
        ))
        .execute(&mut *writer)
        .await
        .unwrap();
    }
    drop(store);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("observation owner retained"))
        .close()
        .await
        .unwrap();
}
