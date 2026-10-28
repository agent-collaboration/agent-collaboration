//! Real admitted sessions submit, fetch, receipt and report one immutable item.
use super::*;
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_conformance::{ConformanceClient, enroll_admitted_peer};
use durable_steps::Uuid;
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ItemId, PeerId};
use message_board::ServiceId;
use serde_json::{Value, json};

pub(super) use super::relay_transport::{operation, relay_request};

pub(super) fn require(actual: Value, expected: Value) -> ConformanceResult<()> {
    if actual != expected {
        return Err(format!("mail relay returned {actual}; expected {expected}").into());
    }
    Ok(())
}

#[tokio::test]
async fn normal_recipient_session_fetches_receipts_and_reports_original_mail()
-> ConformanceResult<()> {
    // A missing Session -> Mailbox route closes the socket before Fetch answers.
    // A wrong peer/installation route returns empty custody instead of this body.
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof=async {
        let client=ConformanceClient::new(server.base_url())?;
        let mut sender=enroll_admitted_peer(&client,"550e8400-e29b-41d4-a716-4466554400e5","019f0000-0000-7000-8000-000000001017","relay-origin","019f0000-0000-7000-8000-000000000e51").await?;
        let mut recipient=enroll_admitted_peer(&client,"550e8400-e29b-41d4-a716-4466554400e6","019f0000-0000-7000-8000-000000001018","relay-recipient","019f0000-0000-7000-8000-000000000e52").await?;
        let proof=async {
            let id=ItemId::new(Uuid::now_v7())?;
            let peer=PeerId::new(recipient.peer)?;
            let service=ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400e6".to_owned())?;
            let item=DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(id,
                serde_json::from_value(json!({"kind":"humanUser","installation":"550e8400-e29b-41d4-a716-4466554400e5"}))?,
                serde_json::from_value(json!({"peer":peer,"address":{"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"recipient"}}))?,None,
                serde_json::from_value(json!({"kind":"message","text":"normal recipient custody","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}))?,
            )?);
            require(operation(&mut sender.socket,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"item":item,"originEpoch":null}}})).await?,
                json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"kind":"submitted","value":{"kind":"accepted","value":{"seq":1,"status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":null}}}}}))?;
            // No receiver room must preserve custody and the fetch prefix.
            let no_room=operation(&mut recipient.socket,relay_request("fetch",json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000,"inboxRoomBytes":0}))).await?;
            require(no_room,json!({"kind":"delivery","value":{"kind":"fetch","value":{"stream":"items","entries":[],"through":0}}}))?;
            let fetched=operation(&mut recipient.socket,relay_request("fetch",json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000}))).await?;
            require(fetched,json!({"kind":"delivery","value":{"kind":"fetch","value":{"stream":"items","entries":[{"kind":"item","value":{"seq":1,"item":item}}],"through":1}}}))?;
            require(operation(&mut recipient.socket,relay_request("received",json!({"stream":"items","through":1}))).await?,
                json!({"kind":"delivery","value":{"kind":"received","value":{"stream":"items","releasedThrough":1}}}))?;
            require(operation(&mut recipient.socket,relay_request("report",json!({"report":{"item":id,"reportSeq":1,"outcome":{"kind":"held"}}}))).await?,
                json!({"kind":"delivery","value":{"kind":"report","value":{"kind":"applied","value":{"item":id,"acknowledged":1,"status":{"kind":"held"},"version":3}}}}))?;
            require(operation(&mut recipient.socket,relay_request("report",json!({"report":{"item":id,"reportSeq":2,"outcome":{"kind":"uncertain"}}}))).await?,
                json!({"kind":"delivery","value":{"kind":"report","value":{"kind":"applied","value":{"item":id,"acknowledged":2,"status":{"kind":"uncertain"},"version":4}}}}))?;
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        let _=sender.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        let _=recipient.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        proof
    }.await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
