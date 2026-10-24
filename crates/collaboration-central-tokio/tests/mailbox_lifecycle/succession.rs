//! Higher-ranked replay moves only the status target, retaining original content.
use super::*;
use fleet_sync_delivery::{PublicationRef, PublicationTarget, resolve_publication};
use fleet_sync_protocol::{CancelBody, DeliveryCancel, StatusVersion};
use relay::{recipient_call, relay};

async fn ranked_submit(
    fixture: &Fixture,
    service: &ServiceId,
    route: &ServiceId,
    peer: PeerId,
    sender: PeerId,
    rank: u64,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> TestResult<Value> {
    let mut envelope: Value =
        serde_json::from_slice(&submit::submit_bytes(service, route, peer, sender, item)?)?;
    envelope
        .pointer_mut("/request")
        .and_then(Value::as_object_mut)
        .ok_or("ranked submit fixture is missing its request object")?
        .insert("rank".into(), json!(rank));
    let answer = fixture
        .runtime
        .on_call(serde_json::to_vec(&envelope)?)
        .await
        .map_err(|error| format!("ranked checked submit: {error:?}"))?;
    Ok(serde_json::from_slice(&answer)?)
}
async fn publication(
    fixture: &Fixture,
    item: ItemId,
    version: u64,
    sender: PeerId,
    route: ServiceId,
) -> TestResult<Value> {
    let reference = PublicationRef {
        item,
        version: StatusVersion::new(version),
        target: PublicationTarget::StatusStream { sender, route },
    };
    Ok(fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let value = resolve_publication::<_, BoardDeliveryTypes>(scope, reference)
                    .await
                    .map_err(StoreError::backend)?;
                serde_json::to_value(value).map_err(StoreError::backend)
            })
        })
        .await?)
}

#[tokio::test]
async fn successor_rank_survives_reopen_and_old_replay_cannot_reclaim_status_or_retirement()
-> TestResult {
    // Checked Registry calls stand in for the already-proven epoch verifier.
    // This case has no pre-fix red; it checks central's rank propagation through
    // real library custody/publication rules, not board handoff or G3 delivery.
    let root = tempfile::tempdir()?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400ea".to_owned())?;
    let old_route = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400eb".to_owned())?;
    let new_route = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400ed".to_owned())?;
    let peer = PeerId::new(Uuid::now_v7())?;
    let old_sender = PeerId::new(Uuid::now_v7())?;
    let new_sender = PeerId::new(Uuid::now_v7())?;
    let id = ItemId::new(Uuid::now_v7())?;
    let item = submit::envelope(
        &service,
        &old_route,
        id,
        peer,
        "immutable old-holder content",
    )?;
    let fixture = Fixture::open(root.path(), &service).await?;
    let proof=async {
        assert_eq!(ranked_submit(&fixture,&service,&old_route,peer,old_sender,1,&item).await?["value"]["kind"],"accepted");
        assert_eq!(ranked_submit(&fixture,&service,&new_route,peer,new_sender,3,&item).await?["value"]["kind"],"replayed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.store.close().await?;
    proof?;
    drop(fixture);
    let fixture = Fixture::open(root.path(), &service).await?;
    let proof=async {
        assert_eq!(ranked_submit(&fixture,&service,&old_route,peer,old_sender,2,&item).await?["value"]["kind"],"replayed");
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("fetch",json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000}))).await?["value"]["entries"],
            json!([{"kind":"item","value":{"seq":1,"item":item}}]));
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("report",json!({"report":{"item":id,"reportSeq":1,"outcome":{"kind":"uncertain"}}}))).await?["value"]["value"]["version"],2);
        assert_eq!(publication(&fixture,id,2,new_sender,new_route.clone()).await?,json!({"item":id,"status":{"kind":"uncertain"},"version":2,"cancel":null}));
        assert_eq!(publication(&fixture,id,1,old_sender,old_route.clone()).await?,json!({"item":id,"status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":null}));
        assert_eq!(relay(&fixture,&service,peer,old_sender,recipient_call("retire",json!({"mailbox":peer,"items":[id]}))).await?,
            json!({"kind":"retire","value":{"retired":[],"unknown":[],"notCurrentTarget":[id]}}));
        assert_eq!(relay(&fixture,&service,peer,new_sender,recipient_call("retire",json!({"mailbox":peer,"items":[id]}))).await?,
            json!({"kind":"retire","value":{"retired":[id],"unknown":[],"notCurrentTarget":[]}}));
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("fetch",json!({"stream":"controls","after":0,"maxItems":10,"maxBytes":1_000_000}))).await?,
            json!({"kind":"fetch","value":{"stream":"controls","entries":[{"kind":"retired","value":{"seq":1,"items":[id]}}],"through":1}}));
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("received",json!({"stream":"controls","through":1}))).await?,
            json!({"kind":"received","value":{"stream":"controls","releasedThrough":1}}));
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("fetch",json!({"stream":"controls","after":1,"maxItems":10,"maxBytes":1_000_000}))).await?["value"]["entries"],json!([]));
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.store.close().await?;
    proof
}

#[tokio::test]
async fn cancel_reserve_fetch_and_receipt_keep_exact_identity_at_mailbox_capacity() -> TestResult {
    // The real library's original item reserves one cancel; a second cancel for
    // that original cannot allocate another Controls sequence. No pre-fix red
    // observed. Effective Router/carrier prevention remains a separate G3 proof.
    let root = tempfile::tempdir()?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400ee".to_owned())?;
    let peer = PeerId::new(Uuid::now_v7())?;
    let sender = PeerId::new(Uuid::now_v7())?;
    let id = ItemId::new(Uuid::now_v7())?;
    let item = submit::envelope(
        &service,
        &service,
        id,
        peer,
        "original with cancellation reserve",
    )?;
    let limits = DeliveryLimits::new::<BoardDeliveryTypes>(
        2_000_000,
        1,
        2_000_000,
        100,
        2_000_000,
        minimum_settlement_allowance::<BoardDeliveryTypes>()?,
    )?;
    let fixture =
        Fixture::open_with_limits(root.path(), "mailbox".into(), &service, limits).await?;
    let proof=async {
        submit::submit(&fixture,&service,&service,peer,sender,&item).await?;
        let cancel=|cancel_id|->TestResult<DeliveryEnvelope<BoardDeliveryTypes>> {
            Ok(DeliveryEnvelope::Cancel(DeliveryCancel::new(cancel_id,
                serde_json::from_value(json!({"kind":"humanUser","installation":service}))?,
                item.recipient().clone(),None,CancelBody {target:id})?))
        };
        let first=cancel(ItemId::new(Uuid::now_v7())?)?;
        let second=cancel(ItemId::new(Uuid::now_v7())?)?;
        assert_eq!(submit::submit(&fixture,&service,&service,peer,sender,&first).await?,
            json!({"kind":"submit","value":{"kind":"accepted","value":{"seq":1,"status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":{"kind":"pending"}}}}));
        assert_eq!(submit::submit(&fixture,&service,&service,peer,sender,&second).await?,
            json!({"kind":"submit","value":{"kind":"cancelAlreadyRequested","value":{"existing":{"item":first.id(),"digest":first.digest()},"status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":{"kind":"pending"}}}}));
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("fetch",json!({"stream":"controls","after":0,"maxItems":10,"maxBytes":1_000_000}))).await?,
            json!({"kind":"fetch","value":{"stream":"controls","entries":[{"kind":"item","value":{"seq":1,"item":first}}],"through":1}}));
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("received",json!({"stream":"controls","through":1}))).await?,
            json!({"kind":"received","value":{"stream":"controls","releasedThrough":1}}));
        assert_eq!(relay(&fixture,&service,peer,sender,recipient_call("query",json!({"mailbox":peer,"items":[first.id(),second.id()]}))).await?["value"]["statuses"],
            json!([{"item":first.id(),"state":{"kind":"known","value":{"digest":first.digest(),"status":{"kind":"atRecipient"},"version":2,"cancel":{"kind":"pending"}}}},{"item":second.id(),"state":{"kind":"unknown"}}]));
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("fetch",json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000}))).await?["value"]["entries"],
            json!([{"kind":"item","value":{"seq":1,"item":item}}]));
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.store.close().await?;
    proof
}
