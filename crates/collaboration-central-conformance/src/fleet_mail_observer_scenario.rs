//! One owner-only custody projection over the same Native and Worker route.
use crate::{ConformanceClient, ConformanceResult, enroll_admitted_peer};
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_core::{
    FLEET_MAIL_CUSTODY_PATH, FleetMailCustodyObservation, FleetMailCustodyRequest,
    FleetMailCustodyRow, FleetMailStatusKind,
};
use fleet_protocol::{DeliveryEnvelope, DeliveryItem, ItemId, PeerId, StatusVersion};
use futures_util::SinkExt;
use reqwest_websocket::{CloseCode, Message, WebSocket};
use serde_json::{Value, json};

const SENDER: &str = "019f0000-0000-7000-8000-000000008d31";
const RECIPIENT: &str = "019f0000-0000-7000-8000-000000008d32";
const CREDENTIAL: &str = "fleet-mail-observer-sender";

pub async fn run_fleet_mail_observer_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let mut sender = enroll_admitted_peer(
        client,
        SENDER,
        "019f0000-0000-7000-8000-000000008d33",
        CREDENTIAL,
        "019f0000-0000-7000-8000-000000008d34",
    )
    .await?;
    let recipient = enroll_admitted_peer(
        client,
        RECIPIENT,
        "019f0000-0000-7000-8000-000000008d35",
        "fleet-mail-observer-recipient",
        "019f0000-0000-7000-8000-000000008d36",
    )
    .await?;
    let proof = async {
        let item = envelope(PeerId::new(recipient.peer)?)?;
        let before = observe(client,item.id()).await?;
        if before != (FleetMailCustodyObservation {outbox_count:0,row:FleetMailCustodyRow::Absent}) {
            return Err(format!("fresh owning Fleet observer fabricated custody: {before:?}").into());
        }
        let unauthorized = client.client.post(format!("{}{FLEET_MAIL_CUSTODY_PATH}",client.base_url))
            .json(&FleetMailCustodyRequest {item:item.id()}).send().await?;
        if unauthorized.status() != reqwest::StatusCode::UNAUTHORIZED {return Err("custody observer admitted missing owner Access".into());}
        let machine = client.with_machine_access(client.client.post(format!("{}{FLEET_MAIL_CUSTODY_PATH}",client.base_url)),CREDENTIAL)?
            .json(&FleetMailCustodyRequest {item:item.id()}).send().await?;
        if machine.status() != reqwest::StatusCode::UNAUTHORIZED {return Err("custody observer admitted machine Access".into());}
        let invalid = client.with_owner_access(client.client.post(format!("{}{FLEET_MAIL_CUSTODY_PATH}",client.base_url)))?
            .json(&json!({"item":"550e8400-e29b-41d4-a716-446655440000"})).send().await?;
        if !invalid.status().is_client_error() {return Err("custody observer did not reject a non-UUIDv7 item at its boundary".into());}
        let accepted = submit(&mut sender.socket,&item).await?;
        if accepted.pointer("/value/value/value") != Some(&json!({"kind":"accepted","value":{
            "status":{"kind":"sent"},"version":1,"cancel":null}})) {
            return Err(format!("observer fixture did not admit actual Sent custody: {accepted}").into());
        }
        let expected = FleetMailCustodyObservation {outbox_count:1,row:FleetMailCustodyRow::Present {
            status_kind:FleetMailStatusKind::Sent,status_version:StatusVersion::new(1),payload_present:true,
        }};
        if observe(client,item.id()).await? != expected || observe(client,item.id()).await? != expected {
            return Err("observer did not read the admitted row's exact status/version/payload/count".into());
        }
        let refused = envelope(PeerId::new(durable_steps::Uuid::now_v7())?)?;
        let answer = submit(&mut sender.socket,&refused).await?;
        if answer.pointer("/value/value/value") != Some(&json!({"kind":"refused","value":{
            "reason":{"kind":"recipientUnknown"}}})) {
            return Err(format!("observer fixture did not reach its canonical refusal: {answer}").into());
        }
        if observe(client,refused.id()).await? != (FleetMailCustodyObservation {outbox_count:1,row:FleetMailCustodyRow::Absent})
            || observe(client,item.id()).await? != expected {
            return Err("refused item changed the Fleet count or admitted custody".into());
        }
        println!("FLEET_MAIL_OBSERVER_CASE owner_only_absent_Sent_v1_payload_count_refusal_unchanged passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = sender.socket.close(CloseCode::Normal, None).await;
    let _ = recipient.socket.close(CloseCode::Normal, None).await;
    proof
}

async fn observe(
    client: &ConformanceClient,
    item: ItemId,
) -> ConformanceResult<FleetMailCustodyObservation> {
    Ok(client
        .with_owner_access(
            client
                .client
                .post(format!("{}{FLEET_MAIL_CUSTODY_PATH}", client.base_url)),
        )?
        .json(&FleetMailCustodyRequest { item })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
fn envelope(peer: PeerId) -> ConformanceResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    Ok(DeliveryEnvelope::Item(DeliveryItem::new(
        ItemId::new(durable_steps::Uuid::now_v7())?,
        serde_json::from_value(json!({"kind":"humanUser","installation":SENDER}))?,
        serde_json::from_value(
            json!({"peer":peer,"address":{"endpoint":{"serviceId":RECIPIENT,
            "endpointId":"codex-local"},"sessionId":"observer-recipient"}}),
        )?,
        None,
        serde_json::from_value(
            json!({"kind":"message","text":"private observer payload must not appear",
            "delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}),
        )?,
    )?))
}
async fn submit(
    socket: &mut WebSocket,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> ConformanceResult<Value> {
    let frame = durable_steps::Uuid::now_v7();
    socket
        .send(Message::Text(
            json!({"kind":"request","value":{"frame":frame,"body":{
        "kind":"consumer","value":{"kind":"mailSubmit","value":{"item":item,"originEpoch":null}}}}})
            .to_string(),
        ))
        .await?;
    let answer = crate::revocation_race_scenarios::receive(socket).await?;
    if answer.pointer("/value/frame") != Some(&json!(frame)) {
        return Err(format!("observer fixture lost exact submit correlation: {answer}").into());
    }
    Ok(answer
        .pointer("/value/body")
        .cloned()
        .ok_or("submit body missing")?)
}
