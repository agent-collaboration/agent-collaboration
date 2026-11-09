//! A lost real Inbox receipt retries one immutable Push across both owning reopens.
use super::{
    grants::connect,
    mail_push_inbox::PushInbox,
    relay::{next_push, pushed, require, submitted, wait_for_receipt},
    *,
};
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_conformance::{
    ConformanceClient, RuntimeControl, RuntimeControlRequest, enroll_admitted_peer,
};
use durable_steps::Uuid;
use fleet_protocol::{
    DeliveryEnvelope, DeliveryItem, ItemId, PeerId, PushResult, ServerCall, Submitter,
};
use reqwest_websocket::CloseCode;
use serde_json::json;
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};
use std::path::Path;
const SENDER: &str = "019f0000-0000-7000-8000-000000004019";
const RECIPIENT: &str = "019f0000-0000-7000-8000-00000000401a";
const SENDER_PEER: &str = "019f0000-0000-7000-8000-000000001019";
const RECIPIENT_PEER: &str = "019f0000-0000-7000-8000-00000000101a";
const SENDER_CREDENTIAL: &str = "restart-mail-origin";
const RECIPIENT_CREDENTIAL: &str = "restart-mail-recipient";
const NOW: i64 = 1_792_281_600_000;
#[derive(Debug, Eq, PartialEq)]
struct CustodyAccounting {
    items: i64,
    payload_bytes: i64,
}
async fn accounting(root: &Path) -> ConformanceResult<CustodyAccounting> {
    // Both native server and all its objects have stopped before this observation.
    let mut reader = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .read_only(true)
        .connect()
        .await?;
    let result=async {
        let row=query::<sqlx_turso::Turso>("SELECT count(*) AS items,coalesce(sum(length(payload)),0) AS payload_bytes FROM _fleet_sync_outbox_items").fetch_one(&mut reader).await?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(CustodyAccounting {items:row.try_get("items")?,payload_bytes:row.try_get("payload_bytes")?})
    }.await;
    reader.close().await?;
    result
}
async fn start_clocked(
    root: &Path,
    jwks: &JwksFixtureServer,
    now: i64,
) -> ConformanceResult<TokioServer> {
    Ok(TokioServer::start_with_controls(
        TokioSettings {
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: "019f0000-0000-7000-8000-000000005240"
                    .to_owned()
                    .try_into()?,
            },
            root: root.into(),
            access_policy: fixture_policy(),
            jwks_url: jwks.url.clone(),
            registry: configuration::registry_config()?,
            holder_answer_timeout: Duration::from_secs(2),
            copy_ack_wait: Duration::from_secs(2),
        },
        vec![RuntimeControlRequest {
            address: collaboration_central_core::ObjectAddress::Fleet,
            command: RuntimeControl::SetTime {
                now: collaboration_central_core::RegistryTime(
                    chrono::DateTime::from_timestamp_millis(now).ok_or("invalid injected clock")?,
                ),
            },
        }],
    )
    .await?)
}
fn roster() -> ConformanceResult<Vec<(message_board::ServiceId, PeerId)>> {
    Ok(vec![(
        SENDER.to_owned().try_into()?,
        SENDER_PEER.to_owned().try_into()?,
    )])
}
#[tokio::test]
async fn whole_server_restart_replays_the_same_push_call_then_receipts_release_once()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let inbox_root = tempfile::tempdir()?;
    let path = inbox_root.path().join("recipient.db");
    let jwks = JwksFixtureServer::start().await?;
    let proof=async {
        let owner:PeerId=RECIPIENT_PEER.to_owned().try_into()?;
        let claim=Submitter::Machine(SENDER_PEER.to_owned().try_into()?);
        let first=start_clocked(root.path(),&jwks,NOW).await?;
        let before=async {
            let client=ConformanceClient::new(first.base_url())?;
            let mut sender=enroll_admitted_peer(&client,SENDER,SENDER_PEER,SENDER_CREDENTIAL,"019f0000-0000-7000-8000-000000000f81").await?;
            let mut recipient=enroll_admitted_peer(&client,RECIPIENT,RECIPIENT_PEER,RECIPIENT_CREDENTIAL,"019f0000-0000-7000-8000-000000000f91").await?;
            let inbox=PushInbox::open(&path,owner,roster()?).await?;
            let result=async {
                let item=DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(ItemId::new(Uuid::now_v7())?,
                    serde_json::from_value(json!({"kind":"humanUser","installation":SENDER}))?,
                    serde_json::from_value(json!({"peer":owner,"address":{"endpoint":{"serviceId":RECIPIENT,"endpointId":"codex-local"},"sessionId":"restart recipient"}}))?,None,
                    serde_json::from_value(json!({"kind":"message","text":"same custody after server restart","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}))?)?);
                require(super::relay::operation(&mut sender.socket,submitted(&item)).await?,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"kind":"submitted","value":{"kind":"accepted","value":{"status":{"kind":"sent"},"version":1,"cancel":null}}}}}))?;
                eprintln!("MAIL_PUSH_REOPEN_STAGE awaiting first Push");
                let (_,call)=next_push(&mut recipient.socket).await?;
                let original_call=serde_json::to_vec(&call)?;
                let ServerCall::Push {submitter,item:offered,..}=call else {return Err("initial request was not Push".into());};
                if submitter!=claim||serde_json::to_vec(&offered)?!=serde_json::to_vec(&item)? {return Err("initial Push changed its sender/body".into());}
                let receipt=inbox.accept(submitter,offered).await?;
                if !matches!(receipt,PushResult::Accepted {..})||inbox.count().await?!=1 {return Err("independent Inbox did not commit fresh custody".into());}
                // The real receipt is deliberately lost before being sent. Fleet
                // must retain its body even though the recipient committed it.
                Ok::<_,Box<dyn std::error::Error+Send+Sync>>((item,original_call))
            }.await;
            let _=sender.socket.close(CloseCode::Normal,None).await;
            let _=recipient.socket.close(CloseCode::Normal,None).await;
            inbox.close()?;result
        }.await;
        first.shutdown().await?;
        eprintln!("MAIL_PUSH_REOPEN_STAGE first owners closed");
        let (item,original_call)=before?;
        let retained=accounting(root.path()).await?;
        if retained.items!=1||retained.payload_bytes<=0 {return Err(format!("lost receipt lost original Fleet body: {retained:?}").into());}
        // Expire the retained SDK drive under injected startup time. No journal
        // edits, sleep, fresh enrollment or changed call identity can recover it.
        let second=start_clocked(root.path(),&jwks,NOW+60_000).await?;
        eprintln!("MAIL_PUSH_REOPEN_STAGE second server started");
        let after=async {
            let inbox=PushInbox::open(&path,owner,roster()?).await?;
            let mut sender=connect(second.base_url(),SENDER_CREDENTIAL,"019f0000-0000-7000-8000-000000000f82").await?;
            let mut recipient=connect(second.base_url(),RECIPIENT_CREDENTIAL,"019f0000-0000-7000-8000-000000000f92").await?;
            eprintln!("MAIL_PUSH_REOPEN_STAGE both credentials reconnected");
            // The push still owed from the first server (in flight on a connection
            // that no longer exists) is resent in the recipient's Welcome catch-up:
            // no clock move, journal edit or new call identity is involved.
            let result=async {
                let (frame,retry)=next_push(&mut recipient).await?;
                eprintln!("MAIL_PUSH_REOPEN_STAGE same-call retry arrived");
                if serde_json::to_vec(&retry)?!=original_call {return Err("whole-server restart changed original Push CallId/sender/body".into());}
                let ServerCall::Push {submitter,item:offered,..}=retry else {return Err("reopened request was not Push".into());};
                let receipt=inbox.accept(submitter,offered).await?;
                if !matches!(receipt,PushResult::Replayed {..})||inbox.count().await?!=1 {return Err("owning Inbox reopen forgot or duplicated custody".into());}
                let reference=fleet_protocol::DeliveryItemRef {item:item.id(),digest:item.digest().clone()};
                let saved=inbox.payload(reference).await?;
                require(serde_json::to_value(saved)?,serde_json::to_value(&item)?.get("value").cloned().ok_or("item value absent")?)?;
                let receipt_bytes=serde_json::to_vec(&receipt)?;
                pushed(&mut recipient,frame,receipt).await?;
                wait_for_receipt(&mut sender,&item,owner).await?;
                pushed(&mut recipient,frame,serde_json::from_slice(&receipt_bytes)?).await?;
                require(super::relay::operation(&mut sender,submitted(&item)).await?,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"kind":"submitted","value":{"kind":"replayed","value":{"status":{"kind":"atRecipient"},"version":2,"cancel":null}}}}}))?;
                Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
            }.await;
            let _=sender.close(CloseCode::Normal,None).await;let _=recipient.close(CloseCode::Normal,None).await;
            inbox.close()?;result
        }.await;
        second.shutdown().await?;after?;
        let released=accounting(root.path()).await?;
        if released!=(CustodyAccounting {items:1,payload_bytes:0}) {return Err(format!("receipt replay lost identity or retained Fleet body: {released:?}").into());}
        println!("MAIL_RESTART_CASE same_Push_and_recipient_Inbox_reopened_then_receipt_releases_once passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let stopped = jwks.shutdown().await;
    proof?;
    stopped?;
    Ok(())
}
