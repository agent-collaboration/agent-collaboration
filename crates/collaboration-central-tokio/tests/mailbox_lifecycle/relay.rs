//! Real custody retains payload until receipt and identity after compaction.
use super::*;
use durable_steps::storage::TransactionScope;
use sqlx_core::{query::query, row::Row};

fn identity() -> Value {
    json!({"operation":Uuid::now_v7(),"step":"relay_request"})
}
pub(super) fn recipient_call(kind: &str, value: Value) -> Value {
    #[derive(serde::Serialize)]
    struct CallFields {
        call: Value,
        #[serde(flatten)]
        fields: Value,
    }
    let value = CallFields {
        call: identity(),
        fields: value,
    };
    json!({"kind":kind,"value":value})
}
pub(super) async fn relay(
    fixture: &Fixture,
    service: &ServiceId,
    recipient: PeerId,
    actor: PeerId,
    request: Value,
) -> TestResult<Value> {
    // Registry is the checked forwarding boundary. Recipient operations below
    // also exercise the Session -> Mailbox edge through a real server test.
    let bytes = serde_json::to_vec(&relay_envelope(service, recipient, actor, request)?)?;
    let answer = fixture
        .runtime
        .on_call(bytes)
        .await
        .map_err(|error| format!("checked mailbox relay did not answer: {error:?}"))?;
    Ok(serde_json::from_slice(&answer)?)
}
fn relay_envelope(
    service: &ServiceId,
    recipient: PeerId,
    actor: PeerId,
    request: Value,
) -> TestResult<Value> {
    let call = request
        .pointer("/value/call")
        .ok_or("relay fixture is missing its CallId")?;
    Ok(json!({
        "source":{"kind":"registry"},"sourceFace":"registry","authority":{"kind":"object"},
        "destination":{"kind":"mailbox","value":service},"destinationFace":"mailbox",
        "identity":call,"kind":"central.mailbox-request",
        "target":{"installation":service,"recipient":recipient},"request":{"actor":actor,"request":request}
    }))
}
async fn physical_payload(fixture: &Fixture) -> TestResult<(i64, i64)> {
    Ok(fixture.object.transaction(|scope| Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT count(*) AS items,coalesce(sum(payload_bytes),0) AS payload FROM _fleet_sync_mail_items")
            .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("items").map_err(StoreError::backend)?,row.try_get("payload").map_err(StoreError::backend)?))
    })).await?)
}
fn fetch() -> Value {
    recipient_call(
        "fetch",
        json!({"stream":"items","after":0,"maxItems":20,"maxBytes":1_000_000}),
    )
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn fetch_retains_held_payload_and_receipt_releases_it_without_forgetting_identity()
-> TestResult {
    // Wrong builds release on Fetch, expire already-fetched Held payload, or
    // forget the immutable id after receipt/retire. Expectations are literal
    // protocol states, checked against the owning database after full reopen.
    let directory = tempfile::tempdir()?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400e2".to_owned())?;
    let origin = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400e1".to_owned())?;
    let peer = PeerId::new(Uuid::now_v7())?;
    let sender = PeerId::new(Uuid::now_v7())?;
    let id = ItemId::new(Uuid::now_v7())?;
    let item = submit::envelope(
        &service,
        &origin,
        id,
        peer,
        "retain the exact original body",
    )?;
    let expected_fetch = json!({"kind":"fetch","value":{"stream":"items","entries":[{"kind":"item","value":{"seq":1,"item":item}}],"through":1}});
    let fixture = Fixture::open(directory.path(), &service).await?;
    let proof = async {
        assert_eq!(submit::submit(&fixture,&service,&origin,peer,sender,&item).await?["value"]["kind"],"accepted");
        assert_eq!(relay(&fixture,&service,peer,peer,fetch()).await?,expected_fetch);
        assert!(physical_payload(&fixture).await?.1 > 0,"Fetch released the live payload");
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("report",json!({"report":{"item":id,"reportSeq":1,"outcome":{"kind":"held"}}}))).await?,
            json!({"kind":"report","value":{"kind":"applied","value":{"item":id,"acknowledged":1,"status":{"kind":"held"},"version":2}}}));
        assert!(physical_payload(&fixture).await?.1 > 0,"Held released the live payload");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.store.close().await?;
    proof?;
    drop(fixture);

    let fixture = Fixture::open(directory.path(), &service).await?;
    // Thirty-one days of wall time does not authorize dropping fetched mail.
    fixture
        .io
        .time
        .store(1_794_960_000_000, std::sync::atomic::Ordering::SeqCst);
    let proof = async {
        assert_eq!(relay(&fixture,&service,peer,peer,fetch()).await?,expected_fetch);
        assert!(physical_payload(&fixture).await?.1 > 0);
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("received",json!({"stream":"items","through":1}))).await?,
            json!({"kind":"received","value":{"stream":"items","releasedThrough":1}}));
        assert_eq!(physical_payload(&fixture).await?,(1,0));
        assert_eq!(submit::submit(&fixture,&service,&origin,peer,sender,&item).await?,
            json!({"kind":"submit","value":{"kind":"replayed","value":{"status":{"kind":"held"},"version":2,"cancel":null}}}));
        let terminal = relay(&fixture,&service,peer,peer,recipient_call("report",json!({"report":{"item":id,"reportSeq":2,"outcome":{"kind":"uncertain"}}}))).await?;
        assert_eq!(terminal,json!({"kind":"report","value":{"kind":"applied","value":{"item":id,"acknowledged":2,"status":{"kind":"uncertain"},"version":3}}}));
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("report",json!({"report":{"item":id,"reportSeq":1,"outcome":{"kind":"held"}}}))).await?,
            json!({"kind":"report","value":{"kind":"applied","value":{"item":id,"acknowledged":1,"status":{"kind":"uncertain"},"version":3}}}));
        let retire = recipient_call("retire",json!({"mailbox":peer,"items":[id]}));
        let retired = json!({"kind":"retire","value":{"retired":[id],"unknown":[],"notCurrentTarget":[]}});
        assert_eq!(relay(&fixture,&service,peer,sender,retire.clone()).await?,retired);
        assert_eq!(relay(&fixture,&service,peer,sender,retire).await?,retired);
        assert_eq!(submit::submit(&fixture,&service,&origin,peer,sender,&item).await?,
            json!({"kind":"submit","value":{"kind":"replayed","value":{"status":{"kind":"uncertain"},"version":3,"cancel":null}}}));
        assert_eq!(relay(&fixture,&service,peer,sender,recipient_call("query",json!({"mailbox":peer,"items":[id]}))).await?,
            json!({"kind":"query","value":{"statuses":[{"item":id,"state":{"kind":"known","value":{"digest":item.digest(),"status":{"kind":"uncertain"},"version":3,"cancel":null}}}]}}));
        assert_eq!(physical_payload(&fixture).await?,(1,0));
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.store.close().await?;
    proof?;
    drop(fixture);
    let fixture = Fixture::open(directory.path(), &service).await?;
    let proof=async {
        assert_eq!(submit::submit(&fixture,&service,&origin,peer,sender,&item).await?,
            json!({"kind":"submit","value":{"kind":"replayed","value":{"status":{"kind":"uncertain"},"version":3,"cancel":null}}}));
        assert_eq!(physical_payload(&fixture).await?,(1,0));
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.store.close().await?;
    proof
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn relay_rejects_foreign_session_identity_and_mailbox_before_touching_custody() -> TestResult
{
    // Hand-built checked-call negatives test the core trust boundary; this is
    // additional coverage with no pre-fix red observed for these exact guards.
    let directory = tempfile::tempdir()?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400e8".to_owned())?;
    let foreign = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400e7".to_owned())?;
    let peer = PeerId::new(Uuid::now_v7())?;
    let stranger = PeerId::new(Uuid::now_v7())?;
    let fixture = Fixture::open(directory.path(), &service).await?;
    let proof = async {
        let item = submit::envelope(
            &service,
            &foreign,
            ItemId::new(Uuid::now_v7())?,
            peer,
            "protected payload",
        )?;
        submit::submit(&fixture, &service, &foreign, peer, stranger, &item).await?;
        let before = physical_payload(&fixture).await?;
        let session = |actor, request| -> TestResult<Value> {
            let mut envelope = relay_envelope(&service, peer, actor, request)?;
            envelope["source"] = json!({"kind":"connection","value":service});
            envelope["sourceFace"] = json!("session");
            Ok(envelope)
        };
        let mut wrong_id = session(peer, fetch())?;
        wrong_id["identity"] = identity();
        let mut wrong_service = session(peer, fetch())?;
        wrong_service["source"]["value"] = json!(foreign);
        let mut wrong_mailbox = relay_envelope(
            &service,
            peer,
            stranger,
            recipient_call("query", json!({"mailbox":stranger,"items":[item.id()]})),
        )?;
        wrong_mailbox["sourceFace"] = json!("registry");
        let mut wrong_target = session(peer, fetch())?;
        wrong_target["target"]["installation"] = json!(foreign);
        for envelope in [
            wrong_id,
            wrong_service,
            wrong_mailbox,
            wrong_target,
            session(stranger, fetch())?,
            session(
                peer,
                recipient_call("retire", json!({"mailbox":peer,"items":[item.id()]})),
            )?,
        ] {
            assert!(
                matches!(
                    fixture
                        .runtime
                        .on_call(serde_json::to_vec(&envelope)?)
                        .await,
                    Err(CallFailure::NotSent { .. })
                ),
                "unchecked relay reached custody: {envelope}"
            );
            assert_eq!(physical_payload(&fixture).await?, before);
        }
        let answer = fixture
            .runtime
            .on_call(serde_json::to_vec(&session(peer, fetch())?)?)
            .await
            .map_err(|error| format!("valid session relay: {error:?}"))?;
        assert_eq!(
            serde_json::from_slice::<Value>(&answer)?["value"]["entries"],
            json!([{"kind":"item","value":{"seq":1,"item":item}}])
        );
        assert_eq!(physical_payload(&fixture).await?, before);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.store.close().await?;
    proof
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn mailbox_capacity_refuses_before_custody_and_keeps_receipt_progress_available() -> TestResult
{
    // This exercises the real item-capacity rule, not a physical disk-full claim.
    let directory = tempfile::tempdir()?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400e4".to_owned())?;
    let origin = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400e3".to_owned())?;
    let peer = PeerId::new(Uuid::now_v7())?;
    let sender = PeerId::new(Uuid::now_v7())?;
    let limits = DeliveryLimits::new::<BoardDeliveryTypes>(
        2_000_000,
        1,
        2_000_000,
        100,
        2_000_000,
        minimum_settlement_allowance::<BoardDeliveryTypes>()?,
    )?;
    let fixture =
        Fixture::open_with_limits(directory.path(), "mailbox".into(), &service, limits).await?;
    let proof = async {
        let first = submit::envelope(&service,&origin,ItemId::new(Uuid::now_v7())?,peer,"first custody")?;
        let second = submit::envelope(&service,&origin,ItemId::new(Uuid::now_v7())?,peer,"must not enter custody")?;
        assert_eq!(submit::submit(&fixture,&service,&origin,peer,sender,&first).await?["value"]["kind"],"accepted");
        assert_eq!(submit::submit(&fixture,&service,&origin,peer,sender,&second).await?,
            json!({"kind":"submit","value":{"kind":"refused","value":{"reason":{"kind":"mailboxFull"}}}}));
        assert_eq!(physical_payload(&fixture).await?.0,1);
        let fetched = relay(&fixture,&service,peer,peer,fetch()).await?;
        assert_eq!(fetched["value"]["entries"],json!([{"kind":"item","value":{"seq":1,"item":first}}]));
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("received",json!({"stream":"items","through":1}))).await?,
            json!({"kind":"received","value":{"stream":"items","releasedThrough":1}}));
        assert_eq!(physical_payload(&fixture).await?,(1,0));
        assert_eq!(relay(&fixture,&service,peer,peer,recipient_call("report",json!({"report":{"item":first.id(),"reportSeq":1,"outcome":{"kind":"uncertain"}}}))).await?["value"]["kind"],"applied");
        assert_eq!(relay(&fixture,&service,peer,sender,recipient_call("retire",json!({"mailbox":peer,"items":[first.id()]}))).await?,
            json!({"kind":"retire","value":{"retired":[first.id()],"unknown":[],"notCurrentTarget":[]}}));
        assert_eq!(submit::submit(&fixture,&service,&origin,peer,sender,&second).await?["value"]["value"]["seq"],2);
        assert_eq!(physical_payload(&fixture).await?.0,2,"compaction must retain the first identity");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.store.close().await?;
    proof
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn checked_settlement_keeps_its_digest_and_refusal_after_reopen_without_allocating_delivery()
-> TestResult {
    // Unknown custody settlement must not become fresh delivery on retry. This
    // exact backend scenario has no pre-fix red; the public library owns the
    // terminal tombstone and conflict decision. Normal remote lookup is pending.
    let root = tempfile::tempdir()?;
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400e9".to_owned())?;
    let peer = PeerId::new(Uuid::now_v7())?;
    let sender = PeerId::new(Uuid::now_v7())?;
    let id = ItemId::new(Uuid::now_v7())?;
    let item = submit::envelope(&service, &service, id, peer, "must remain settled")?;
    let fixture = Fixture::open(root.path(), &service).await?;
    let proof = relay(
        &fixture,
        &service,
        peer,
        sender,
        recipient_call(
            "settle",
            json!({"mailbox":peer,"item":id,"digest":item.digest()}),
        ),
    )
    .await;
    fixture.store.close().await?;
    assert_eq!(
        proof?,
        json!({"kind":"settle","value":{"kind":"settled","value":{"reason":"custodyUnresolvable","version":1}}})
    );
    drop(fixture);
    let fixture = Fixture::open(root.path(), &service).await?;
    let proof=async {
        let refusal=json!({"kind":"refused","value":{"reason":{"kind":"processingFailed","value":{"kind":"custodyUnresolvable"}}}});
        assert_eq!(relay(&fixture,&service,peer,sender,recipient_call("settle",json!({"mailbox":peer,"item":id,"digest":item.digest()}))).await?,
            json!({"kind":"settle","value":{"kind":"replayed","value":{"status":refusal,"version":1,"cancel":null}}}));
        assert_eq!(submit::submit(&fixture,&service,&service,peer,sender,&item).await?,
            json!({"kind":"submit","value":{"kind":"replayed","value":{"status":refusal,"version":1,"cancel":null}}}));
        let conflict=submit::envelope(&service,&service,id,peer,"different body")?;
        assert_eq!(relay(&fixture,&service,peer,sender,recipient_call("settle",json!({"mailbox":peer,"item":id,"digest":conflict.digest()}))).await?,
            json!({"kind":"settle","value":{"kind":"itemConflict","value":{"existing":item.digest()}}}));
        assert_eq!(relay(&fixture,&service,peer,peer,fetch()).await?,json!({"kind":"fetch","value":{"stream":"items","entries":[],"through":0}}));
        assert_eq!(physical_payload(&fixture).await?,(1,0));
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.store.close().await?;
    proof
}
