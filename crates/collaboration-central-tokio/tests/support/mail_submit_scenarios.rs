//! Mail authority is checked against real registry enrollment and epoch history.

use board_sync_protocol::{BoardConsumerProtocol, BoardConsumerRequest, BoardDeliveryTypes};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
};
use fleet_sync_protocol::{
    CancelBody, ClientFrame, DeliveryCancel, DeliveryEnvelope, DeliveryItem, Epoch, FrameId,
    ItemId, OperationRequest, PeerId, Recipient,
};
use futures_util::{SinkExt, StreamExt};
use message_board::ServiceId;
use reqwest_websocket::{Message, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;

const RECIPIENT: &str = "550e8400-e29b-41d4-a716-4466554400c2";

pub(super) fn project_envelope(
    project: durable_steps::Uuid,
    peer: PeerId,
    cancel: bool,
) -> ConformanceResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    let sender = serde_json::from_value(json!({"kind":"project","project":project}))?;
    let recipient: Recipient<_> = serde_json::from_value(json!({"peer":peer,"address":{
        "endpoint":{"serviceId":RECIPIENT,"endpointId":"codex-local"},"sessionId":"mail recipient"
    }}))?;
    let id = ItemId::new(durable_steps::Uuid::now_v7())?;
    Ok(if cancel {
        DeliveryEnvelope::Cancel(DeliveryCancel::new(
            id,
            sender,
            recipient,
            None,
            CancelBody {
                target: ItemId::new(durable_steps::Uuid::now_v7())?,
            },
        )?)
    } else {
        let target = durable_steps::Uuid::now_v7();
        let body = serde_json::from_value(
            json!({"kind":"notification","excerpt":"Authority probe",
            "pointer":{"project":project,"subject":{"kind":"task","id":target},"position":{"epoch":1,"position":1}},
            "source":{"kind":"intent","intent":target}}),
        )?;
        DeliveryEnvelope::Item(DeliveryItem::new(id, sender, recipient, None, body)?)
    })
}

pub(super) async fn receive(socket: &mut WebSocket) -> ConformanceResult<Value> {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await?
        .ok_or("mail authority session ended before its correlated answer")??;
    match message {
        Message::Text(text) => Ok(serde_json::from_str(&text)?),
        Message::Binary(bytes) => Ok(serde_json::from_slice(&bytes)?),
        other => Err(format!("expected mail authority answer, got {other:?}").into()),
    }
}

pub(crate) async fn run_ungranted_mail_refusal(url: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(url)?;
    let mut origin = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-4466554400c1",
        "019f0000-0000-7000-8000-000000001025",
        "mail-project-origin",
        "019f0000-0000-7000-8000-000000000c11",
    )
    .await?;
    let recipient = enroll_admitted_peer(
        &client,
        RECIPIENT,
        "019f0000-0000-7000-8000-000000001026",
        "mail-project-recipient",
        "019f0000-0000-7000-8000-000000000c12",
    )
    .await?;
    let peer = PeerId::new(recipient.peer)?;
    let installation = ServiceId::try_from(RECIPIENT.to_owned())?;
    let project = durable_steps::Uuid::now_v7();
    let proof = async {
        for cancel in [false, true] {
            let item = project_envelope(project, peer, cancel)?;
            let request =
                BoardConsumerRequest::mail_submit(item, Some(Epoch::new(1)?), &installation, peer)?;
            let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
            origin
                .socket
                .send(Message::Text(serde_json::to_string(&ClientFrame::<
                    BoardConsumerProtocol,
                >::Request {
                    frame,
                    body: OperationRequest::Consumer(request),
                })?))
                .await?;
            let answer = receive(&mut origin.socket).await?;
            if answer
                != json!({"kind":"answer","value":{"frame":frame,"body":{"kind":"consumer","value":{
                    "kind":"mailSubmit","value":{"kind":"notWriter"}
                }}}})
            {
                return Err(
                    format!("never-granted project mail was not NotWriter: {answer}").into(),
                );
            }
        }
        println!(
            "MAIL_SUBMIT_CASE never_granted_epoch_refuses_item_and_cancel_on_normal_session passed"
        );
        // A wire sender can pair a real peer with another enrolled address.
        let mismatched = project_envelope(project, PeerId::new(origin.peer)?, false)?;
        let forged = DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(
            ItemId::new(durable_steps::Uuid::now_v7())?,
            serde_json::from_value(json!({"kind":"humanUser","installation":RECIPIENT}))?,
            serde_json::from_value(json!({"peer":peer,"address":{
                "endpoint":{"serviceId":RECIPIENT,"endpointId":"codex-local"},"sessionId":"recipient"
            }}))?, None,
            serde_json::from_value(json!({"kind":"message","text":"forged origin","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}))?,
        )?);
        for (item, epoch, field, case) in [
            (mismatched, Some(1), "recipient.address.endpoint.serviceId", "recipient_peer_and_installation_must_share_one_registry_row"),
            (forged, None, "sender", "declared_sender_cannot_replace_authenticated_origin"),
        ] {
            let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
            origin.socket.send(Message::Text(json!({"kind":"request","value":{
                "frame":frame,"body":{"kind":"consumer","value":{"kind":"mailSubmit","value":{
                    "item":item,"originEpoch":epoch
                }}}
            }}).to_string())).await?;
            let answer = receive(&mut origin.socket).await?;
            if answer != json!({"kind":"answer","value":{"frame":frame,"body":{
                "kind":"invalidShape","value":{"field":field}
            }}}) { return Err(format!("mail admission accepted {case}: {answer}").into()); }
            println!("MAIL_SUBMIT_CASE {case} passed");
        }
        origin.socket.send(Message::Text(json!({"kind":"hello","value":{
            "sessionId":"019f0000-0000-7000-8000-000000000c11","schemaSupport":{"maxVersion":1}
        }}).to_string())).await?;
        if receive(&mut origin.socket).await? != json!({"kind":"welcome","value":{
            "sessionId":"019f0000-0000-7000-8000-000000000c11","databases":[]
        }}) {return Err("mail admission refusals made the session unusable".into());}
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let origin_closed = origin
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let recipient_closed = recipient
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    origin_closed?;
    recipient_closed?;
    Ok(())
}
