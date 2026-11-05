//! Sender controls run through admitted sockets and actual Fleet custody.
use super::relay_transport as relay;
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardDeliveryTypes, MailSubmitAnswer,
};
use collaboration_central_conformance::ConformanceResult;
use collaboration_central_conformance::{ConformanceClient, enroll_admitted_peer};
use durable_steps::Uuid;
use fleet_sync_protocol::{
    DeliveryAnswer, DeliveryEnvelope, DeliveryFailureKind, DeliveryItem, DeliveryRefusal,
    DeliveryStatus, ItemId, ItemStatusState, OperationAnswer, PeerId, ServerCall, ServerFrame,
    ServerRequest, SettleResult, SubmitResult, Submitter,
};
use futures_util::StreamExt;
use reqwest_websocket::{Message, WebSocket};
use serde_json::json;
use std::time::Duration;

fn item(peer: PeerId, service: &str) -> ConformanceResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    Ok(DeliveryEnvelope::Item(
        DeliveryItem::<BoardDeliveryTypes>::new(
            ItemId::new(Uuid::now_v7())?,
            serde_json::from_value(
                json!({"kind":"humanUser","installation":"550e8400-e29b-41d4-a716-446655440525"}),
            )?,
            serde_json::from_value(
                json!({"peer":peer,"address":{"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"sender-controls-recipient"}}),
            )?,
            None,
            serde_json::from_value(
                json!({"kind":"message","text":"sender controls immutable custody","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}),
            )?,
        )?,
    ))
}
async fn request(
    socket: &mut WebSocket,
    kind: &str,
    value: serde_json::Value,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    Ok(serde_json::from_value(
        relay::operation(socket, relay::relay_request(kind, value)).await?,
    )?)
}
async fn submit(
    socket: &mut WebSocket,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    Ok(serde_json::from_value(relay::operation(socket,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"item":item,"originEpoch":null}}})).await?)?)
}

async fn receive_push(
    socket: &mut WebSocket,
) -> ConformanceResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("recipient ended before its Push")??;
            let frame: ServerFrame<BoardConsumerProtocol> = match message {
                Message::Text(text) => serde_json::from_str(&text)?,
                Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
                other => return Err(format!("recipient expected Push data, got {other:?}").into()),
            };
            match frame {
                ServerFrame::Request {
                    body: ServerRequest::Crate(ServerCall::Push { item, .. }),
                    ..
                } => return Ok(item),
                ServerFrame::Notice { .. } => {}
                _ => {
                    return Err(
                        "recipient received another operation instead of the original Push".into(),
                    );
                }
            }
        }
    })
    .await?
}

pub(super) async fn raw_submit_is_refused_without_custody_and_the_same_socket_remains_usable(
    base_url: &str,
) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base_url)?;
    let mut sender = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-446655440525",
        "019f0000-0000-7000-8000-000000000525",
        "raw-submit-origin",
        "019f0000-0000-7000-8000-000000005251",
    )
    .await?;
    let mut recipient = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-446655440526",
        "019f0000-0000-7000-8000-000000000526",
        "raw-submit-recipient",
        "019f0000-0000-7000-8000-000000005252",
    )
    .await?;
    let proof = async {
        let recipient_peer = PeerId::new(recipient.peer)?;
        let item = item(recipient_peer, "550e8400-e29b-41d4-a716-446655440526")?;
        if !matches!(request(&mut sender.socket, "submit", json!({"item":item})).await?, OperationAnswer::InvalidShape { field } if field == "operation") {
            return Err("raw Submit bypassed the board provenance gate".into());
        }
        let OperationAnswer::Delivery(DeliveryAnswer::StatusFetch(statuses)) = request(&mut sender.socket, "statusFetch", json!({"after":0,"limit":10})).await? else {
            return Err("raw Submit refusal damaged the admitted sender socket".into());
        };
        if !statuses.events.is_empty() || statuses.through.value() != 0 {
            return Err("raw Submit created a sender status effect".into());
        }
        let OperationAnswer::Delivery(DeliveryAnswer::Query(queried)) = request(&mut sender.socket, "query", json!({"mailbox":recipient_peer,"items":[item.id()]})).await? else {
            return Err("raw Submit refusal damaged sender Query".into());
        };
        if !matches!(queried.statuses.as_slice(), [status] if status.item == item.id() && matches!(status.state, ItemStatusState::Unknown)) {
            return Err("raw Submit established Fleet custody".into());
        }
        // This correlated recipient barrier also rejects any unexpected Push;
        // the shared transport never discards Push frames as incidental traffic.
        let OperationAnswer::Delivery(DeliveryAnswer::StatusFetch(recipient_statuses)) = request(&mut recipient.socket, "statusFetch", json!({"after":0,"limit":10})).await? else {
            return Err("recipient barrier lost its typed answer".into());
        };
        if !recipient_statuses.events.is_empty() || recipient_statuses.through.value() != 0 {
            return Err("raw Submit created a recipient status effect".into());
        }
        Ok(())
    }.await;
    let _ = sender
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let _ = recipient
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof
}

pub(super) async fn sender_query_and_settlement_preserve_existing_custody_and_pin_unknown_identity(
    base_url: &str,
) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base_url)?;
    let mut sender = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-446655440525",
        "019f0000-0000-7000-8000-000000000525",
        "settlement-origin",
        "019f0000-0000-7000-8000-000000005253",
    )
    .await?;
    let mut recipient = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-446655440526",
        "019f0000-0000-7000-8000-000000000526",
        "settlement-recipient",
        "019f0000-0000-7000-8000-000000005254",
    )
    .await?;
    let proof = async {
        let recipient_peer = PeerId::new(recipient.peer)?;
        let existing = item(recipient_peer, "550e8400-e29b-41d4-a716-446655440526")?;
        let submitter=Submitter::Machine(PeerId::new(sender.peer)?);
        let unknown = item(recipient_peer, "550e8400-e29b-41d4-a716-446655440526")?;
        if !matches!(submit(&mut sender.socket, &existing).await?, OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(MailSubmitAnswer::Submitted(SubmitResult::Accepted { .. })))) {
            return Err("valid board submit did not establish real custody".into());
        }
        let query = json!({"mailbox":recipient_peer,"items":[existing.id(),unknown.id()]});
        let OperationAnswer::Delivery(DeliveryAnswer::Query(before)) = request(&mut sender.socket, "query", query.clone()).await? else {
            return Err("sender Query did not reach Fleet custody".into());
        };
        if !matches!(before.statuses.as_slice(), [known, absent]
            if known.item == existing.id() && matches!(&known.state, ItemStatusState::Known {digest,status:DeliveryStatus::Sent,..} if digest == existing.digest())
            && absent.item == unknown.id() && matches!(absent.state, ItemStatusState::Unknown)) {
            return Err("Query changed existing custody or fabricated unknown custody".into());
        }
        if !matches!(request(&mut sender.socket, "settle", json!({"mailbox":recipient_peer,"submitter":submitter,"item":existing.id(),"digest":existing.digest()})).await?, OperationAnswer::Delivery(DeliveryAnswer::Settle(SettleResult::Replayed {status:DeliveryStatus::Sent,..}))) {
            return Err("Settle replaced known custody with a tombstone".into());
        }
        if !matches!(request(&mut sender.socket, "settle", json!({"mailbox":recipient_peer,"submitter":submitter,"item":unknown.id(),"digest":unknown.digest()})).await?, OperationAnswer::Delivery(DeliveryAnswer::Settle(SettleResult::Settled {reason:DeliveryFailureKind::CustodyUnresolvable,..}))) {
            return Err("Settle did not establish the relay-owned unknown outcome".into());
        }
        if !matches!(submit(&mut sender.socket, &unknown).await?, OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(MailSubmitAnswer::Submitted(SubmitResult::Replayed {status:DeliveryStatus::Refused {reason:DeliveryRefusal::ProcessingFailed {kind:DeliveryFailureKind::CustodyUnresolvable}},..})))) {
            return Err("late Submit reopened a settled unknown identity".into());
        }
        let pushed = receive_push(&mut recipient.socket).await?;
        if serde_json::to_value(&pushed)? != serde_json::to_value(&existing)? {
            return Err("settlement altered the queued original Push or delivered the tombstone".into());
        }
        // Do not manufacture a recipient receipt. The known item stays Sent
        // while the body is only observed on the socket, before Inbox custody.
        let OperationAnswer::Delivery(DeliveryAnswer::Query(after)) = request(&mut sender.socket, "query", query).await? else {
            return Err("post-Push sender Query lost its typed answer".into());
        };
        if !matches!(after.statuses.as_slice(), [known, settled]
            if known.item == existing.id() && matches!(&known.state, ItemStatusState::Known {digest,status:DeliveryStatus::Sent,..} if digest == existing.digest())
            && settled.item == unknown.id() && matches!(&settled.state, ItemStatusState::Known {digest,status:DeliveryStatus::Refused {reason:DeliveryRefusal::ProcessingFailed {kind:DeliveryFailureKind::CustodyUnresolvable}},..} if digest == unknown.digest())) {
            return Err("socket write was treated as custody or settlement identity changed".into());
        }
        Ok(())
    }.await;
    let _ = sender
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let _ = recipient
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof
}
