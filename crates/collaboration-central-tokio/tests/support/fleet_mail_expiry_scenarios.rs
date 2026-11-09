//! Real alarm expiry is observed before any status read or recipient connection.
use super::relay_transport::{operation, relay_request};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, RuntimeControl, RuntimeControlRequest,
    enroll_admitted_peer, owner_assertion,
};
use collaboration_central_core::ObjectAddress;
use fleet_protocol::{
    DeliveryEnvelope, DeliveryItem, DeliveryStatus, FrameId, ItemId, PeerId, ServerCall,
    ServerFrame, ServerRequest, StatusPublication, StatusVersion, Submitter,
};
use futures_util::StreamExt;
use reqwest_websocket::{CloseCode, Message, WebSocket};
use runtime_primitives::SyncTime;
use serde_json::{Value, json};
use std::time::Duration;

const NOW: i64 = 1_792_281_600_000;
const SENDER_SERVICE: &str = "019f0000-0000-7000-8000-000000001071";
pub(super) const RECIPIENT_SERVICE: &str = "019f0000-0000-7000-8000-000000001072";
const SENDER_PEER: &str = "019f0000-0000-7000-8000-000000002071";
const RECIPIENT_PEER: &str = "019f0000-0000-7000-8000-000000002072";
type Acceptance = <BoardDeliveryTypes as fleet_protocol::DeliveryTypes>::Acceptance;

async fn set_time(base: &str, millis: i64) -> ConformanceResult<()> {
    reqwest::Client::new()
        .post(format!("{base}/admin/v1/conformance/runtime"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .timeout(Duration::from_secs(10))
        .json(&RuntimeControlRequest {
            address: ObjectAddress::Fleet,
            command: RuntimeControl::SetTime {
                now: collaboration_central_core::RegistryTime(
                    chrono::DateTime::from_timestamp_millis(millis)
                        .ok_or("expiry fixture clock is outside the UTC range")?,
                ),
            },
        })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn enroll_offline(base: &str) -> ConformanceResult<()> {
    let credential = "shared-expiry-recipient";
    let hash = ring::digest::digest(&ring::digest::SHA256, credential.as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let response: Value = reqwest::Client::new()
        .post(format!("{base}/admin/v1/machines/enroll"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .timeout(Duration::from_secs(10))
        .json(&json!({"kind":"enrollMachine","value":{
            "label":credential,"installation":RECIPIENT_SERVICE,"peer":RECIPIENT_PEER,
            "access_client_id":collaboration_central_conformance::access_client_id(credential)?,
            "credential_hash":hash,"expires_at":"2033-05-18T03:33:20Z"
        }}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if response.get("kind") != Some(&json!("machineEnrolled"))
        || response.pointer("/value/peer") != Some(&json!(RECIPIENT_PEER))
    {
        return Err(format!("offline enrollment changed its independent peer: {response}").into());
    }
    Ok(())
}

fn envelope(
    expires_at: Option<SyncTime>,
) -> ConformanceResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    Ok(DeliveryEnvelope::Item(DeliveryItem::new(
        ItemId::new(durable_steps::Uuid::now_v7())?,
        serde_json::from_value(json!({"kind":"humanUser","installation":SENDER_SERVICE}))?,
        serde_json::from_value(json!({"peer":RECIPIENT_PEER,"address":{
            "endpoint":{"serviceId":RECIPIENT_SERVICE,"endpointId":"codex-local"},
            "sessionId":"019f0000-0000-7000-8000-000000012072"
        }}))?,
        expires_at,
        serde_json::from_value(json!({"kind":"message","text":"offline alarm expiry",
            "delivery":"queue","guard":null,"senderDisplayName":null,"replyTo":null}))?,
    )?))
}

async fn submit(
    socket: &mut WebSocket,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> ConformanceResult<()> {
    let answer = operation(socket,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"item":item,"originEpoch":null}}})).await?;
    if answer.pointer("/value/value/kind") != Some(&json!("submitted"))
        || answer.pointer("/value/value/value")
            != Some(
                &json!({"kind":"accepted","value":{"status":{"kind":"sent"},"version":1,"cancel":null}}),
            )
    {
        return Err(format!("expiry admission lost its canonical Sent snapshot: {answer}").into());
    }
    Ok(())
}

async fn next_request(
    socket: &mut WebSocket,
) -> ConformanceResult<(FrameId, ServerCall<BoardDeliveryTypes>)> {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let message = socket.next().await.ok_or("expiry socket ended")??;
            let bytes = match message {
                Message::Text(text) => text.into_bytes(),
                Message::Binary(bytes) => bytes.to_vec(),
                other => return Err(format!("expiry received {other:?}").into()),
            };
            match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                ServerFrame::Request {
                    frame,
                    body: ServerRequest::Crate(call),
                } => return Ok((frame, call)),
                ServerFrame::Notice { .. } => {}
                _ => return Err("expiry oracle received an uncorrelated frame".into()),
            }
        }
    })
    .await?
}

async fn observe_unanswered_resend(
    base: &str,
    socket: &mut WebSocket,
    frame: FrameId,
    sent: &ServerCall<BoardDeliveryTypes>,
) -> ConformanceResult<()> {
    // The recipient never answers this Push. It stays owed: after its retry
    // time (30 s) it is sent again, with a new send id and the same call.
    set_time(base, NOW + 31_000 + 61_000).await?;
    let (resent_frame, resent) = next_request(socket).await?;
    if resent_frame == frame {
        return Err("the unanswered Push was resent under its old send id".into());
    }
    if serde_json::to_vec(&resent)? != serde_json::to_vec(sent)? {
        return Err("the resent Push changed its call identity, sender or body".into());
    }
    println!("FLEET_MAIL_EXPIRY_CASE unanswered_Push_resent_after_its_retry_time passed");
    Ok(())
}

pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    set_time(base, NOW).await?;
    // HTTP enrollment supplies identity but never opens a B socket. Therefore
    // no attempted Push can accidentally establish MayHaveReached before TTL.
    enroll_offline(base).await?;
    let mut sender = enroll_admitted_peer(
        &client,
        SENDER_SERVICE,
        SENDER_PEER,
        "shared-expiry-sender",
        "019f0000-0000-7000-8000-000000011071",
    )
    .await?;
    let proof = async {
        let item = envelope(Some(SyncTime::from_unix_millis(NOW + 1_000)))?;
        submit(&mut sender.socket, &item).await?;
        // This existing control wakes the real platform alarm; it does not call
        // the SDK expiry helper or read status. No recipient has connected.
        // Push expiry is a due source held by the 30 s batch cadence, so the
        // item expiring at NOW + 1 s is published at the next batch slot.
        set_time(base, NOW + 31_000).await?;
        let (_, ServerCall::PublishStatus { publication, .. }) =
            next_request(&mut sender.socket).await?
        else {
            return Err("offline expiry did not publish unsolicited status".into());
        };
        let expected = StatusPublication::<Acceptance> {
            item: item.id(),
            status: DeliveryStatus::Expired,
            version: StatusVersion::new(2),
            cancel: None,
        };
        if publication != expected {
            return Err(
                format!("offline alarm published another revision: {publication:?}").into(),
            );
        }
        // A Machine live hint needs no invented custody ACK. Its independently
        // fetchable event must contain exactly the same committed revision.
        let fetched = operation(
            &mut sender.socket,
            relay_request("statusFetch", json!({"after":0,"limit":10})),
        )
        .await?;
        let events = fetched
            .pointer("/value/value/events")
            .and_then(Value::as_array)
            .ok_or("expiry status stream omitted events")?;
        let [event] = events.as_slice() else {
            return Err("expiry stream did not contain exactly its one event".into());
        };
        if event.get("item") != Some(&json!(item.id()))
            || event.get("status") != Some(&json!({"kind":"expired"}))
            || event.get("version") != Some(&json!(2))
            || event.get("cancel") != Some(&Value::Null)
            || event.get("cursor") != Some(&json!(1))
            || fetched.pointer("/value/value/through") != Some(&json!(1))
        {
            return Err(
                format!("expiry stream differs from its live publication: {fetched}").into(),
            );
        }

        let mut recipient = enroll_admitted_peer(
            &client,
            RECIPIENT_SERVICE,
            RECIPIENT_PEER,
            "shared-expiry-recipient",
            "019f0000-0000-7000-8000-000000011072",
        )
        .await?;
        let recipient_proof = async {
            // Correlated read cannot silently discard an expired Push. A later
            // sentinel proves that the real B transport can now receive mail.
            operation(
                &mut recipient.socket,
                relay_request("statusFetch", json!({"after":0,"limit":10})),
            )
            .await?;
            let sentinel = envelope(None)?;
            submit(&mut sender.socket, &sentinel).await?;
            let (frame, call) = next_request(&mut recipient.socket).await?;
            let ServerCall::Push {
                submitter,
                item: pushed,
                ..
            } = &call
            else {
                return Err("reconnected recipient did not receive the sentinel Push".into());
            };
            if *submitter != Submitter::Machine(PeerId::try_from(SENDER_PEER.to_owned())?)
                || serde_json::to_vec(pushed)? != serde_json::to_vec(&sentinel)?
            {
                return Err(
                    "expired body reached B, or the sentinel lost its verified sender/bytes".into(),
                );
            }
            observe_unanswered_resend(base, &mut recipient.socket, frame, &call).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let closed = recipient.socket.close(CloseCode::Normal, None).await;
        recipient_proof?;
        closed?;
        println!(
            "FLEET_MAIL_EXPIRY_CASE offline_Push_wait_Expired_before_recipient_reconnect passed"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let _ = sender.socket.close(CloseCode::Normal, None).await;
    proof
}
