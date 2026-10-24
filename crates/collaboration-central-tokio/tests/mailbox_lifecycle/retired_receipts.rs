//! Retired delivery receipts preserve identity without retaining the mail payload.
use super::*;
use durable_steps::storage::TransactionScope;
use fleet_sync_protocol::{DeliveryStatus, ItemDigest};
use sqlx_core::{query::query, row::Row};

type BoardAcceptance = <BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Acceptance;

#[derive(Debug, Eq, PartialEq)]
struct CompactReceipt {
    item: ItemId,
    stored_item: ItemId,
    digest: ItemDigest,
    status: DeliveryStatus<BoardAcceptance>,
    version: i64,
    payload_kind: Option<String>,
    payload_bytes: i64,
    live: i64,
    capacity_slot: i64,
    settlement_bytes: i64,
    kind: String,
}

async fn compact_receipt(fixture: &Fixture, id: ItemId) -> TestResult<CompactReceipt> {
    let key = id.as_uuid().to_string();
    Ok(fixture.object.transaction(move |scope| Box::pin(async move {
        // Observe only the needed fields; the library remains the custody codec owner.
        let row = query::<sqlx_turso::Turso>(
            "SELECT item_id, json_extract(record, '$.kind') AS kind, json_extract(record, '$.value.id') AS stored_item, json_extract(record, '$.value.digest') AS digest, json_extract(record, '$.value.status') AS status, json_extract(record, '$.value.version') AS version, json_type(record, '$.value.payload') AS payload_kind, payload_bytes, live, capacity_slot, settlement_bytes FROM _fleet_sync_mail_items WHERE item_id = ?"
        ).bind(key).fetch_optional(scope.executor()).await.map_err(StoreError::backend)?
            .ok_or_else(|| StoreError::backend("compacted receipt missing after retirement"))?;
        let item: String = row.try_get("item_id").map_err(StoreError::backend)?;
        let stored_item: String = row.try_get("stored_item").map_err(StoreError::backend)?;
        let digest: String = row.try_get("digest").map_err(StoreError::backend)?;
        let status: String = row.try_get("status").map_err(StoreError::backend)?;
        Ok(CompactReceipt {
            item: ItemId::new(Uuid::parse_str(&item).map_err(StoreError::backend)?).map_err(StoreError::backend)?,
            stored_item: ItemId::new(Uuid::parse_str(&stored_item).map_err(StoreError::backend)?).map_err(StoreError::backend)?,
            digest: serde_json::from_value(Value::String(digest)).map_err(StoreError::backend)?,
            status: serde_json::from_str(&status).map_err(StoreError::backend)?,
            version: row.try_get("version").map_err(StoreError::backend)?,
            payload_kind: row.try_get("payload_kind").map_err(StoreError::backend)?,
            payload_bytes: row.try_get("payload_bytes").map_err(StoreError::backend)?,
            live: row.try_get("live").map_err(StoreError::backend)?,
            capacity_slot: row.try_get("capacity_slot").map_err(StoreError::backend)?,
            settlement_bytes: row.try_get("settlement_bytes").map_err(StoreError::backend)?,
            kind: row.try_get("kind").map_err(StoreError::backend)?,
        })
    })).await?)
}

#[derive(Debug, Eq, PartialEq)]
struct MailboxEffects {
    items: i64,
    controls: i64,
    publications: i64,
    status_events: i64,
    outbox: i64,
    operations: i64,
    call_receipts: i64,
    sequence_state: String,
}

async fn mailbox_effects(fixture: &Fixture) -> TestResult<MailboxEffects> {
    Ok(fixture.object.transaction(|scope| Box::pin(async move {
        let row = query::<sqlx_turso::Turso>(
            "SELECT (SELECT count(*) FROM _fleet_sync_mail_items) AS items, (SELECT count(*) FROM _fleet_sync_mail_controls) AS controls, (SELECT count(*) FROM _fleet_sync_status_owed) AS publications, (SELECT count(*) FROM _fleet_sync_status_events) AS status_events, (SELECT count(*) FROM _fleet_sync_outbox_items) AS outbox, (SELECT count(*) FROM _fleet_sync_operations) AS operations, (SELECT count(*) FROM _fleet_sync_delivery_receipts) AS call_receipts, (SELECT record FROM _fleet_sync_mail_state WHERE singleton = 1) AS sequence_state"
        ).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(MailboxEffects {
            items: row.try_get("items").map_err(StoreError::backend)?,
            controls: row.try_get("controls").map_err(StoreError::backend)?,
            publications: row.try_get("publications").map_err(StoreError::backend)?,
            status_events: row.try_get("status_events").map_err(StoreError::backend)?,
            outbox: row.try_get("outbox").map_err(StoreError::backend)?,
            operations: row.try_get("operations").map_err(StoreError::backend)?,
            call_receipts: row.try_get("call_receipts").map_err(StoreError::backend)?,
            sequence_state: row.try_get("sequence_state").map_err(StoreError::backend)?,
        })
    })).await?)
}

#[tokio::test]
async fn delivered_retired_mail_keeps_compact_identity_and_replays_without_new_effects()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400f2".to_owned())?;
    let origin = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400f1".to_owned())?;
    let recipient = PeerId::new(Uuid::now_v7())?;
    let sender = PeerId::new(Uuid::now_v7())?;
    let id = ItemId::new(Uuid::now_v7())?;
    let item = submit::envelope(&service, &origin, id, recipient, "synthetic delivered mail")?;
    let delivered = json!({"kind":"delivered","value":{"acceptance":"started"}});
    let fixture = Fixture::open(directory.path(), &service).await?;
    let first = async {
        assert_eq!(submit::submit(&fixture, &service, &origin, recipient, sender, &item).await?,
            json!({"kind":"submit","value":{"kind":"accepted","value":{"seq":1,"status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":null}}}));
        assert_eq!(relay::relay(&fixture,&service,recipient,recipient,relay::recipient_call("fetch",json!({"stream":"items","after":0,"maxItems":20,"maxBytes":1_000_000}))).await?,
            json!({"kind":"fetch","value":{"stream":"items","entries":[{"kind":"item","value":{"seq":1,"item":item}}],"through":1}}));
        assert_eq!(relay::relay(&fixture,&service,recipient,recipient,relay::recipient_call("received",json!({"stream":"items","through":1}))).await?,
            json!({"kind":"received","value":{"stream":"items","releasedThrough":1}}));
        assert_eq!(relay::relay(&fixture,&service,recipient,recipient,relay::recipient_call("report",json!({"report":{"item":id,"reportSeq":1,"outcome":delivered}}))).await?,
            json!({"kind":"report","value":{"kind":"applied","value":{"item":id,"acknowledged":1,"status":delivered,"version":3}}}));
        assert_eq!(relay::relay(&fixture,&service,recipient,sender,relay::recipient_call("retire",json!({"mailbox":recipient,"items":[id]}))).await?,
            json!({"kind":"retire","value":{"retired":[id],"unknown":[],"notCurrentTarget":[]}}));
        let compacted = compact_receipt(&fixture, id).await?;
        assert_eq!(compacted.item, id);
        assert_eq!(compacted.stored_item, id);
        assert_eq!(&compacted.digest, item.digest());
        assert_eq!(compacted.status, serde_json::from_value::<DeliveryStatus<BoardAcceptance>>(delivered.clone())?);
        assert_eq!(compacted.version, 3);
        assert_eq!(compacted.kind, "compacted");
        assert_eq!(compacted.payload_kind, None);
        assert_eq!((compacted.payload_bytes, compacted.live, compacted.capacity_slot, compacted.settlement_bytes), (0,0,0,0));
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(compacted)
    }.await;
    fixture.store.close().await?;
    let compacted = first?;
    drop(fixture);

    let fixture = Fixture::open(directory.path(), &service).await?;
    let proof = async {
        assert_eq!(compact_receipt(&fixture, id).await?, compacted);
        let before = mailbox_effects(&fixture).await?;
        assert_eq!(before.items, 1);
        assert_eq!(before.controls, 1);
        // submit() supplies a new CallId: this must be item-identity replay.
        assert_eq!(submit::submit(&fixture,&service,&origin,recipient,sender,&item).await?,
            json!({"kind":"submit","value":{"kind":"replayed","value":{"status":delivered,"version":3,"cancel":null}}}));
        assert_eq!(compact_receipt(&fixture, id).await?, compacted);
        assert_eq!(mailbox_effects(&fixture).await?, before);
        let conflicting = submit::envelope(&service,&origin,id,recipient,"changed synthetic digest")?;
        assert_ne!(conflicting.digest(), item.digest());
        assert_eq!(submit::submit(&fixture,&service,&origin,recipient,sender,&conflicting).await?,
            json!({"kind":"submit","value":{"kind":"refused","value":{"reason":{"kind":"itemConflict","value":{"existing":item.digest()}}}}}));
        assert_eq!(compact_receipt(&fixture, id).await?, compacted);
        assert_eq!(mailbox_effects(&fixture).await?, before);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    fixture.store.close().await?;
    proof
}
