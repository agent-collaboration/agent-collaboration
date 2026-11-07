//! O11(b): committed revocation refuses new custody before EndSessions arrives.
use crate::revocation_race_scenarios::{control, live, observed, receive};
use crate::{ConformanceClient, ConformanceResult, enroll_admitted_peer, owner_assertion};
use collaboration_central_core::RevocationControl;
use futures_util::SinkExt;
use reqwest_websocket::{CloseCode, Message, WebSocket};
use serde_json::{Value, json};
use tokio::task::JoinSet;

const SENDER: &str = "550e8400-e29b-41d4-a716-4466554400e9";
const SESSION: &str = "019f0000-0000-7000-8000-000000000983";

pub async fn run_sender_revocation_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let mut sender = enroll_admitted_peer(
        client,
        SENDER,
        "019f0000-0000-7000-8000-000000001008",
        "revoked-mail-sender",
        SESSION,
    )
    .await?;
    let service = durable_steps::Uuid::now_v7().to_string();
    let mut recipient = enroll_admitted_peer(
        client,
        &service,
        &durable_steps::Uuid::now_v7().to_string(),
        "revoked-mail-recipient",
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await?;
    let mut requests = JoinSet::new();
    let proof = async {
        control(client, RevocationControl::ArmDelivery).await?;
        let caller = client.clone();
        requests.spawn(async move {
            Ok::<Value, Box<dyn std::error::Error + Send + Sync>>(caller.client
                .post(format!("{}/admin/v1/machines/revoke", caller.base_url))
                .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
                .json(&json!({"kind":"revokeMachine","value":{"installation":SENDER}}))
                .send().await?.error_for_status()?.json().await?)
        });
        let held = observed(client, |value| value.gate_armed && value.gate_reached
            && value.enrollment.as_ref().is_some_and(|row| row.revoked)
            && matches!(value.operation.as_ref().map(|operation| &operation.state),
                Some(durable_steps::OperationState::Running { .. }))).await?;
        // A normal Hello proves that this is the still-attached old session.
        live(&mut sender.socket, SESSION).await?;
        let item = fleet_protocol::DeliveryEnvelope::<board_sync_protocol::BoardDeliveryTypes>::Item(
            fleet_protocol::DeliveryItem::new(
                fleet_protocol::ItemId::new(durable_steps::Uuid::now_v7())?,
                serde_json::from_value(json!({"kind":"humanUser","installation":SENDER}))?,
                serde_json::from_value(json!({"peer":recipient.peer,"address":{
                    "endpoint":{"serviceId":service,"endpointId":"codex-local"},
                    "sessionId":"revocation-recipient"}}))?, None,
                serde_json::from_value(json!({"kind":"message","text":"must never enter custody",
                    "delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}))?)?);
        let cancel = fleet_protocol::DeliveryEnvelope::<board_sync_protocol::BoardDeliveryTypes>::Cancel(
            fleet_protocol::DeliveryCancel::new(
                fleet_protocol::ItemId::new(durable_steps::Uuid::now_v7())?,
                item.sender().clone(), item.recipient().clone(), None,
                fleet_protocol::CancelBody { target: item.id() })?);
        for envelope in [item, cancel] {
            let answer = operation(&mut sender.socket, json!({"kind":"consumer","value":{
                "kind":"mailSubmit","value":{"item":envelope,"originEpoch":null}}})).await?;
            let expected = json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{
                "kind":"submitted","value":{"kind":"refused","value":{"reason":{"kind":"senderNotEnrolled"}}}}}});
            if answer != expected {
                return Err(format!("revoked live sender admitted mail: {answer}; expected {expected}").into());
            }
            if control(client, RevocationControl::Observe).await?.outbox_items != held.outbox_items {
                return Err("refused revoked sender changed Fleet outbox count".into());
            }
        }
        // The correlated recipient barrier rejects any Push instead of silently
        // skipping it, and requires no timeout to claim absence.
        let barrier = operation(&mut recipient.socket, json!({"kind":"delivery","value":{
            "kind":"statusFetch","value":{"call":{"operation":durable_steps::Uuid::now_v7(),
            "step":"revoked_sender_barrier"},"after":0,"limit":10}}})).await?;
        if barrier != json!({"kind":"delivery","value":{"kind":"statusFetch","value":{"events":[],"through":0}}}) {
            return Err(format!("refused sender produced recipient traffic: {barrier}").into());
        }
        println!("SENDER_ENROLLMENT_CASE revoked_live_session_item_and_cancel_refused_outbox_unchanged_no_Push passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let released = control(client, RevocationControl::ReleaseDelivery).await;
    requests.abort_all();
    while requests.join_next().await.is_some() {}
    let _ = sender.socket.close(CloseCode::Normal, None).await;
    let _ = recipient.socket.close(CloseCode::Normal, None).await;
    proof?;
    released?;
    Ok(())
}

async fn operation(socket: &mut WebSocket, body: Value) -> ConformanceResult<Value> {
    let frame = durable_steps::Uuid::now_v7();
    socket
        .send(Message::Text(
            json!({"kind":"request","value":{"frame":frame,"body":body}}).to_string(),
        ))
        .await?;
    let answer = receive(socket).await?;
    if answer.get("kind") != Some(&json!("answer"))
        || answer.pointer("/value/frame") != Some(&json!(frame))
    {
        return Err(format!("expected exact correlated answer, got {answer}").into());
    }
    Ok(answer
        .pointer("/value/body")
        .cloned()
        .ok_or("missing answer body")?)
}
