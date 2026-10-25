//! A shared normal-session oracle observes offline TTL before any recipient Fetch.
use super::relay_transport::{operation, relay_request};
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ItemId, PeerId};
use reqwest_websocket::{CloseCode, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;

async fn latest(socket: &mut WebSocket, item: ItemId, expected: Value) -> ConformanceResult<Value> {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let answer = operation(
                socket,
                relay_request("statusFetch", json!({"after":0,"limit":10})),
            )
            .await?;
            if let Some(event) = answer
                .pointer("/value/value/events")
                .and_then(Value::as_array)
                .and_then(|events| {
                    events
                        .iter()
                        .rev()
                        .find(|event| event.get("item") == Some(&json!(item)))
                })
                && event.get("status") == Some(&expected)
            {
                return Ok(event.clone());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?
}
pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    let sender_service = "550e8400-e29b-41d4-a716-446655441071";
    let recipient_service = "550e8400-e29b-41d4-a716-446655441072";
    let mut sender = enroll_admitted_peer(
        &client,
        sender_service,
        "019f0000-0000-7000-8000-000000001071",
        "shared-expiry-sender",
        "019f0000-0000-7000-8000-000000011071",
    )
    .await?;
    let recipient = enroll_admitted_peer(
        &client,
        recipient_service,
        "019f0000-0000-7000-8000-000000001072",
        "shared-expiry-recipient",
        "019f0000-0000-7000-8000-000000011072",
    )
    .await?;
    let peer = recipient.peer;
    recipient.socket.close(CloseCode::Normal, None).await?;
    let proof=async {
        let item_id=ItemId::new(durable_steps::Uuid::now_v7())?;
        let item=DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(item_id,
            serde_json::from_value(json!({"kind":"humanUser","installation":sender_service}))?,
            serde_json::from_value(json!({"peer":PeerId::new(peer)?,"address":{"endpoint":{"serviceId":recipient_service,"endpointId":"codex-local"},"sessionId":"quiet recipient"}}))?,
            Some(runtime_primitives::SyncTime::from_unix_millis(chrono::Utc::now().timestamp_millis()+1500)),
            serde_json::from_value(json!({"kind":"message","text":"offline timer expiry","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}))?,
        )?);
        let submitted=operation(&mut sender.socket,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"item":item,"originEpoch":null}}})).await?;
        if submitted.pointer("/value/value/kind")!=Some(&json!("submitted")) {return Err(format!("quiet mail not admitted: {submitted}").into());}
        let sent=latest(&mut sender.socket,item_id,json!({"kind":"sent","value":{"seq":1}})).await?;
        if sent.get("version")!=Some(&json!(1)) {return Err("initial offline Sent version changed".into());}
        let expired=latest(&mut sender.socket,item_id,json!({"kind":"expired"})).await?;
        if expired.get("version")!=Some(&json!(2)) || expired.get("cursor").and_then(Value::as_u64)<=sent.get("cursor").and_then(Value::as_u64) {return Err("offline expiry lost its newer exact revision".into());}
        // Only after sender status proves expiry do we reconnect and fetch.
        let mut recipient=enroll_admitted_peer(&client,recipient_service,"019f0000-0000-7000-8000-000000001072","shared-expiry-recipient","019f0000-0000-7000-8000-000000011073").await?;
        let fetched=operation(&mut recipient.socket,relay_request("fetch",json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000}))).await;
        recipient.socket.close(CloseCode::Normal,None).await?;
        if fetched?!=json!({"kind":"delivery","value":{"kind":"fetch","value":{"stream":"items","entries":[],"through":1}}}) {return Err("expired mail retained its payload".into());}
        println!("MAILBOX_DEADLINE_CASE offline_timer_newer_Expired_and_released_payload passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = sender.socket.close(CloseCode::Normal, None).await;
    proof
}
