//! The checked Registry -> Mailbox call reaches real custody and survives reopen.
use super::*;
use durable_steps::storage::TransactionScope;
use sqlx_core::{query::query, row::Row};

pub(super) fn envelope(
    service: &ServiceId,
    origin: &ServiceId,
    id: ItemId,
    peer: PeerId,
    text: &str,
) -> TestResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    Ok(DeliveryEnvelope::Item(DeliveryItem::new(
        id,
        serde_json::from_value(json!({"kind":"humanUser","installation":origin}))?,
        Recipient {
            peer,
            address: serde_json::from_value(
                json!({"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"submit recipient"}),
            )?,
        },
        None,
        serde_json::from_value(
            json!({"kind":"message","text":text,"delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}),
        )?,
    )?))
}
pub(super) async fn submit(
    fixture: &Fixture,
    service: &ServiceId,
    origin: &ServiceId,
    peer: PeerId,
    sender: PeerId,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> TestResult<Value> {
    let bytes = fixture
        .runtime
        .on_call(submit_bytes(service, origin, peer, sender, item)?)
        .await
        .map_err(|error| format!("checked mailbox submit did not answer: {error:?}"))?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub(super) fn submit_bytes(
    service: &ServiceId,
    origin: &ServiceId,
    peer: PeerId,
    sender: PeerId,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(&json!({
        "source":{"kind":"registry"},"sourceFace":"registry","authority":{"kind":"object"},
        "destination":{"kind":"mailbox","value":service},"destinationFace":"mailbox",
        "identity":{"operation":Uuid::now_v7(),"step":"submit"},"kind":"central.mailbox-submit",
        "target":{"installation":service,"recipient":peer},"request":{"origin":sender,"route":origin,"rank":null,"item":item}
    }))?)
}
async fn counts(fixture: &Fixture) -> TestResult<(i64, i64, i64)> {
    Ok(fixture.object.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM _fleet_sync_mail_items) AS items,(SELECT count(*) FROM _fleet_sync_status_owed) AS owed,(SELECT count(*) FROM _fleet_sync_operations) AS operations").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("items").map_err(StoreError::backend)?,row.try_get("owed").map_err(StoreError::backend)?,row.try_get("operations").map_err(StoreError::backend)?))
    })).await?)
}

async fn operation_kinds(fixture: &Fixture) -> TestResult<Vec<String>> {
    Ok(fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>("SELECT kind FROM _fleet_sync_operations ORDER BY kind")
                    .fetch_all(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .into_iter()
                    .map(|row| row.try_get("kind").map_err(StoreError::backend))
                    .collect()
            })
        })
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn checked_submit_commits_one_custody_item_and_publication_before_replay_after_reopen()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400d2".to_owned())?;
    let origin = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400d1".to_owned())?;
    let peer = PeerId::try_from("019f0000-0000-7000-8000-000000000d02".to_owned())?;
    let sender = PeerId::try_from("019f0000-0000-7000-8000-000000000d01".to_owned())?;
    let id = ItemId::new(Uuid::now_v7())?;
    let item = envelope(&service, &origin, id, peer, "canonical submitted body")?;
    let fixture = Fixture::open(directory.path(), &service).await?;
    let first=async {
        let answer=submit(&fixture,&service,&origin,peer,sender,&item).await?;
        assert_eq!(answer,json!({"kind":"submit","value":{"kind":"accepted","value":{"seq":1,"status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":null}}}));
        assert_eq!(counts(&fixture).await?,(1,1,2));
        assert_eq!(operation_kinds(&fixture).await?, vec![
            "central.mailbox-notice", "fleet-sync.delivery.publish-status"
        ]);
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.store.close().await?;
    first?;
    drop(fixture);
    let fixture = Fixture::open(directory.path(), &service).await?;
    let proof=async {
        let replay=submit(&fixture,&service,&origin,peer,sender,&item).await?;
        assert_eq!(replay,json!({"kind":"submit","value":{"kind":"replayed","value":{"status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":null}}}));
        assert_eq!(counts(&fixture).await?,(1,1,2));
        let conflicting=envelope(&service,&origin,id,peer,"different body")?;
        let conflict=submit(&fixture,&service,&origin,peer,sender,&conflicting).await?;
        assert_eq!(conflict,json!({"kind":"submit","value":{"kind":"refused","value":{"reason":{"kind":"itemConflict","value":{"existing":item.digest()}}}}}));
        assert_eq!(counts(&fixture).await?,(1,1,2));
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.store.close().await?;
    proof
}

async fn custody_record(fixture: &Fixture, replacement: Option<String>) -> TestResult<String> {
    Ok(fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>("SELECT record FROM _fleet_sync_mail_items")
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                let saved: String = row.try_get("record").map_err(StoreError::backend)?;
                if let Some(replacement) = replacement {
                    query::<sqlx_turso::Turso>("UPDATE _fleet_sync_mail_items SET record = ?")
                        .bind(replacement)
                        .execute(scope.executor())
                        .await
                        .map_err(StoreError::backend)?;
                }
                Ok(saved)
            })
        })
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn corrupt_custody_rolls_back_and_keeps_its_owner_unhealthy_until_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400d4".to_owned())?;
    let origin = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400d3".to_owned())?;
    let peer = PeerId::try_from("019f0000-0000-7000-8000-000000000d04".to_owned())?;
    let sender = PeerId::try_from("019f0000-0000-7000-8000-000000000d03".to_owned())?;
    let item = envelope(
        &service,
        &origin,
        ItemId::new(Uuid::now_v7())?,
        peer,
        "stored custody",
    )?;
    let later = envelope(
        &service,
        &origin,
        ItemId::new(Uuid::now_v7())?,
        peer,
        "must await a healthy owner",
    )?;
    let fixture = Fixture::open(directory.path(), &service).await?;
    let proof = async {
        assert_eq!(
            submit(&fixture, &service, &origin, peer, sender, &item).await?["value"]["kind"],
            "accepted"
        );
        // Damage the real persisted record; retain its bytes without duplicating
        // the library's private custody representation in this test.
        let original = custody_record(&fixture, Some("{}".to_owned())).await?;
        let unavailable = json!({"kind":"mailboxUnavailable","value":{"reason":{
            "kind":"processingFailed","value":{"kind":"custodyCorrupt"}
        }}});
        assert_eq!(
            submit(&fixture, &service, &origin, peer, sender, &item).await?,
            unavailable
        );
        assert_eq!(counts(&fixture).await?, (1, 1, 2));
        assert_eq!(custody_record(&fixture, Some(original)).await?, "{}");
        // Repairing storage cannot revive an owner already marked unhealthy.
        assert_eq!(
            submit(&fixture, &service, &origin, peer, sender, &later).await?,
            unavailable
        );
        assert_eq!(counts(&fixture).await?, (1, 1, 2));
        assert!(fixture.runtime.on_alarm().await?.completed.is_empty());
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.store.close().await?;
    proof?;
    drop(fixture);
    let reopened = Fixture::open(directory.path(), &service).await?;
    let proof = async {
        assert_eq!(
            submit(&reopened, &service, &origin, peer, sender, &later).await?,
            json!({"kind":"submit","value":{"kind":"accepted","value":{
                "seq":2,"status":{"kind":"sent","value":{"seq":2}},"version":1,"cancel":null
            }}})
        );
        assert_eq!(counts(&reopened).await?, (2, 2, 4));
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    reopened.store.close().await?;
    proof
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn publication_admission_failure_rolls_back_custody_then_wake_loss_replays_one_commit()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400d6".to_owned())?;
    let origin = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400d5".to_owned())?;
    let peer = PeerId::try_from("019f0000-0000-7000-8000-000000000d06".to_owned())?;
    let sender = PeerId::try_from("019f0000-0000-7000-8000-000000000d05".to_owned())?;
    let item = envelope(
        &service,
        &origin,
        ItemId::new(Uuid::now_v7())?,
        peer,
        "atomic custody and admission",
    )?;
    let fixture = Fixture::open(directory.path(), &service).await?;
    let proof = async {
        fixture.object.transaction(|scope| Box::pin(async move {
            sqlx_core::raw_sql::raw_sql("CREATE TRIGGER fail_publication_admission BEFORE INSERT ON _fleet_sync_operations BEGIN SELECT RAISE(ABORT, 'controlled publication storage failure'); END")
                .execute(scope.executor()).await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        assert!(matches!(fixture.runtime.on_call(submit_bytes(&service,&origin,peer,sender,&item)?).await, Err(CallFailure::AnswerLost)));
        assert_eq!(counts(&fixture).await?,(0,0,0),"publication failure committed partial custody");
        fixture.object.transaction(|scope| Box::pin(async move {
            query::<sqlx_turso::Turso>("DROP TRIGGER fail_publication_admission").execute(scope.executor()).await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        fixture.io.fail_wake.store(true,std::sync::atomic::Ordering::SeqCst);
        assert!(matches!(fixture.runtime.on_call(submit_bytes(&service,&origin,peer,sender,&item)?).await, Err(CallFailure::AnswerLost)));
        assert_eq!(counts(&fixture).await?,(1,1,2),"wake loss discarded the committed custody decision");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    fixture.store.close().await?;
    proof?;
    drop(fixture);
    let reopened = Fixture::open(directory.path(), &service).await?;
    let proof = async {
        assert_eq!(
            submit(&reopened, &service, &origin, peer, sender, &item).await?,
            json!({"kind":"submit","value":{"kind":"replayed","value":{
                "status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":null
            }}})
        );
        assert_eq!(counts(&reopened).await?, (1, 1, 2));
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    reopened.store.close().await?;
    proof
}
