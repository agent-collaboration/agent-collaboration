//! Whole-server restart preserves fetched custody and cumulative receipt identity.
use super::{
    grants::{connect, start},
    relay::{operation, relay_request, require},
    *,
};
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_conformance::{ConformanceClient, enroll_admitted_peer};
use durable_steps::Uuid;
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ItemId, PeerId};
use reqwest_websocket::CloseCode;
use serde_json::json;
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};
use std::path::Path;

const SENDER: &str = "550e8400-e29b-41d4-a716-4466554400f8";
const RECIPIENT: &str = "550e8400-e29b-41d4-a716-4466554400f9";
const SENDER_CREDENTIAL: &str = "restart-mail-origin";
const RECIPIENT_CREDENTIAL: &str = "restart-mail-recipient";

#[derive(Debug, Eq, PartialEq)]
struct CustodyAccounting {
    items: i64,
    payload_bytes: i64,
}
async fn accounting(root: &Path) -> ConformanceResult<CustodyAccounting> {
    // Every server and object has stopped before this read-only connection opens.
    let mut reader = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("mailbox-{RECIPIENT}.db")))
        .read_only(true)
        .connect()
        .await?;
    let proof=async {
        let row=query::<sqlx_turso::Turso>("SELECT count(*) AS items,coalesce(sum(payload_bytes),0) AS payload_bytes FROM _fleet_sync_mail_items")
            .fetch_one(&mut reader).await?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(CustodyAccounting {
            items:row.try_get("items")?,payload_bytes:row.try_get("payload_bytes")?,
        })
    }.await;
    reader.close().await?;
    proof
}
#[tokio::test]
async fn whole_server_restart_replays_the_same_fetch_call_then_receipts_release_once()
-> ConformanceResult<()> {
    // Wrong startup code that forgets durable custody loses the original Fetch
    // body after restart. A receipt that releases twice changes the canonical
    // AtRecipient version or forgets identity. Literal protocol values are the
    // oracle; real HTTP, sessions, object stores and full shutdown stay in path.
    // This is additional proof of existing code; no pre-fix red is claimed.
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let proof=async {
        let first=start(root.path(),&jwks).await?;
        let before=async {
            let client=ConformanceClient::new(first.base_url())?;
            let mut sender=enroll_admitted_peer(&client,SENDER,"019f0000-0000-7000-8000-000000001019",SENDER_CREDENTIAL,"019f0000-0000-7000-8000-000000000f81").await?;
            let mut recipient=enroll_admitted_peer(&client,RECIPIENT,"019f0000-0000-7000-8000-00000000101a",RECIPIENT_CREDENTIAL,"019f0000-0000-7000-8000-000000000f91").await?;
            let before=async {
                let item=DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(
                    ItemId::new(Uuid::now_v7())?,
                    serde_json::from_value(json!({"kind":"humanUser","installation":SENDER}))?,
                    serde_json::from_value(json!({"peer":PeerId::new(recipient.peer)?,"address":{"endpoint":{"serviceId":RECIPIENT,"endpointId":"codex-local"},"sessionId":"restart recipient"}}))?,
                    None,serde_json::from_value(json!({"kind":"message","text":"same custody after server restart","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}))?,
                )?);
                let submit=json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"item":item,"originEpoch":null}}});
                require(operation(&mut sender.socket,submit.clone()).await?,
                    json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"kind":"submitted","value":{"kind":"accepted","value":{"seq":1,"status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":null}}}}}))?;
                let fetch=relay_request("fetch",json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000}));
                let expected=json!({"kind":"delivery","value":{"kind":"fetch","value":{"stream":"items","entries":[{"kind":"item","value":{"seq":1,"item":item}}],"through":1}}});
                require(operation(&mut recipient.socket,fetch.clone()).await?,expected.clone())?;
                Ok::<_,Box<dyn std::error::Error+Send+Sync>>((submit,fetch,expected))
            }.await;
            let _=sender.socket.close(CloseCode::Normal,None).await;
            let _=recipient.socket.close(CloseCode::Normal,None).await;
            before
        }.await;
        first.shutdown().await?;
        let (submit,fetch,expected)=before?;
        let retained=accounting(root.path()).await?;
        if retained.items!=1 || retained.payload_bytes<=0 {
            return Err(format!("Fetch did not retain custody before the whole restart: {retained:?}").into());
        }
        eprintln!("MAIL_RESTART_CASE first_server_stopped_payload_retained");

        let second=start(root.path(),&jwks).await?;
        let after=async {
            // Reuse the saved enrollment credentials; do not re-enroll or open
            // a fresh mailbox to manufacture the identity after restart.
            let mut sender=connect(second.base_url(),SENDER_CREDENTIAL,"019f0000-0000-7000-8000-000000000f82").await?;
            let mut recipient=connect(second.base_url(),RECIPIENT_CREDENTIAL,"019f0000-0000-7000-8000-000000000f92").await?;
            let after=async {
                require(operation(&mut recipient,fetch).await?,expected)?;
                eprintln!("MAIL_RESTART_CASE same_call_fetch_replayed_after_restart");
                let received=relay_request("received",json!({"stream":"items","through":1}));
                let receipt=json!({"kind":"delivery","value":{"kind":"received","value":{"stream":"items","releasedThrough":1}}});
                require(operation(&mut recipient,received.clone()).await?,receipt.clone())?;
                require(operation(&mut recipient,received).await?,receipt)?;
                require(operation(&mut sender,submit).await?,
                    json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"kind":"submitted","value":{"kind":"replayed","value":{"status":{"kind":"atRecipient"},"version":2,"cancel":null}}}}}))?;
                Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
            }.await;
            let _=sender.close(CloseCode::Normal,None).await;
            let _=recipient.close(CloseCode::Normal,None).await;
            after
        }.await;
        second.shutdown().await?;
        after?;
        let released=accounting(root.path()).await?;
        if released!=(CustodyAccounting {items:1,payload_bytes:0}) {
            return Err(format!("receipt replay lost identity or retained payload: {released:?}").into());
        }
        eprintln!("MAIL_RESTART_CASE receipt_replayed_once_identity_retained_payload_released");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    jwks.shutdown().await?;
    proof
}
