//! Shared real owner enrollment and normal session directory/read/submit contract.

use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardDeliveryTypes,
    BoardReadPage, BoardReadScope, InstallationPeerLookup, MAX_INSTALLATION_PEERS_PAGE_ROWS,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, access_client_id, enroll_admitted_peer, owner_assertion,
};
use fleet_sync_protocol::{
    ClientFrame, DeliveryEnvelope, DeliveryItem, DeliveryRefusal, FrameId, ItemId, OperationAnswer,
    OperationRequest, PeerId, ServerFrame, SubmitResult,
};
use futures_util::{SinkExt, StreamExt};
use message_board::ServiceId;
use reqwest_websocket::{Message, WebSocket};
use ring::digest::{SHA256, digest};
use serde_json::{Value, json};
use std::time::Duration;

const SENDER_SERVICE: &str = "01a20000-0000-7000-9000-ffffffffffff";
const SENDER_PEER: &str = "01a20000-0000-7000-8000-ffffffffffff";

async fn owner_call(
    http: &reqwest::Client,
    base: &str,
    path: &str,
    body: &Value,
) -> ConformanceResult<Value> {
    Ok(http
        .post(format!("{base}{path}"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

async fn answered(
    socket: &mut WebSocket,
    frame: FrameId,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let bytes = match socket.next().await.ok_or("directory session ended")?? {
                Message::Binary(bytes) => bytes.to_vec(),
                Message::Text(text) => text.into_bytes(),
                message => return Err(format!("unexpected directory frame: {message:?}").into()),
            };
            match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                ServerFrame::Answer {
                    frame: actual,
                    body,
                } if actual == frame => return Ok(body),
                ServerFrame::Notice { .. } => {}
                _ => return Err("directory answer lost frame correlation".into()),
            }
        }
    })
    .await?
}

async fn request(
    socket: &mut WebSocket,
    request: BoardConsumerRequest,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Consumer(request),
            })?
            .into(),
        ))
        .await?;
    answered(socket, frame).await
}

fn read(scope: BoardReadScope) -> BoardConsumerRequest {
    BoardConsumerRequest::Read {
        scope,
        cursor: None,
    }
}

pub(super) async fn run(base_url: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base_url)?;
    let mut sender = enroll_admitted_peer(
        &client,
        SENDER_SERVICE,
        SENDER_PEER,
        "directory-reader",
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await?;
    let proof = async {
        let http = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(20)).build()?;
        let mut expected = Vec::new();
        // One more row than the fixed page bound forces an actual continuation.
        for index in 0..=MAX_INSTALLATION_PEERS_PAGE_ROWS {
            let service: ServiceId = format!("019f0100-0000-7000-9000-{index:012x}").try_into()?;
            let peer: PeerId = format!("019f0100-0000-7000-8000-{index:012x}").try_into()?;
            let credential = format!("directory-row-{index}");
            let enrolled = owner_call(&http, base_url, "/admin/v1/machines/enroll", &json!({
                "kind":"enrollMachine","value":{
                    "installation":service,"peer":peer,"label":credential,
                    "access_client_id":access_client_id(&credential)?,
                    "credential_hash":digest(&SHA256, credential.as_bytes()).as_ref().iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
                    "expires_at":"2033-05-18T03:33:20Z"
                }
            })).await?;
            assert_eq!(enrolled.pointer("/value/peer"), Some(&json!(peer)), "{enrolled}");
            expected.push((service, peer));
        }
        expected.push((SENDER_SERVICE.to_owned().try_into()?, SENDER_PEER.to_owned().try_into()?));
        let (revoked_service, revoked_peer) = expected.first().cloned().ok_or("empty enrollment fixture")?;
        let revoked = owner_call(&http, base_url, "/admin/v1/machines/revoke", &json!({
            "kind":"revokeMachine","value":{"installation":revoked_service}
        })).await?;
        assert_eq!(revoked, json!({"kind":"machineRevoked"}));
        let first = request(&mut sender.socket, read(BoardReadScope::InstallationPeers { after: None })).await?;
        let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::InstallationPeers(first))) = first
        else { return Err("first directory page was not typed".into()); };
        assert_eq!(first.peers(), expected.iter().take(MAX_INSTALLATION_PEERS_PAGE_ROWS).cloned().collect::<Vec<_>>());
        assert!(first.peers().contains(&(revoked_service.clone(), revoked_peer)), "revoked immutable binding disappeared");
        let after = first.next().ok_or("full page omitted continuation")?;
        assert_eq!(Some(after), first.peers().last().map(|pair| pair.1));
        let second = request(&mut sender.socket, read(BoardReadScope::InstallationPeers { after: Some(after) })).await?;
        let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::InstallationPeers(second))) = second
        else { return Err("continued directory page was not typed".into()); };
        assert_eq!(second.peers(), expected.iter().skip(MAX_INSTALLATION_PEERS_PAGE_ROWS).cloned().collect::<Vec<_>>());
        second.validate_after(Some(after))?;
        assert_eq!(second.next(), None);
        println!("INSTALLATION_DIRECTORY_CASE two_pages_exact_native_order_and_revoked_binding");
        let lookup = request(&mut sender.socket, read(BoardReadScope::InstallationPeer { service: revoked_service.clone() })).await?;
        assert!(matches!(lookup, OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::InstallationPeer(InstallationPeerLookup::Enrolled { peer }))) if peer == revoked_peer));
        let unknown = request(&mut sender.socket, read(BoardReadScope::InstallationPeer {
            service: "019f0100-0000-7000-9000-ffffffffffff".to_owned().try_into()?,
        })).await?;
        assert!(matches!(unknown, OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::InstallationPeer(InstallationPeerLookup::Unknown)))));
        let terminal = request(&mut sender.socket, read(BoardReadScope::InstallationPeers {
            after: Some("ffffffff-ffff-7fff-bfff-ffffffffffff".to_owned().try_into()?),
        })).await?;
        assert!(matches!(terminal, OperationAnswer::InvalidShape { field } if field == "after"));
        let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
        let mut malformed = serde_json::to_value(ClientFrame::<BoardConsumerProtocol>::Request {
            frame, body: OperationRequest::Consumer(read(BoardReadScope::InstallationPeers { after: None })),
        })?;
        *malformed.pointer_mut("/value/body/value/value/scope/value/after").ok_or("typed after field moved")? = json!("not-a-peer");
        sender.socket.send(Message::Binary(serde_json::to_vec(&malformed)?.into())).await?;
        assert!(matches!(answered(&mut sender.socket, frame).await?, OperationAnswer::InvalidShape { field } if field == "body"));
        println!("INSTALLATION_DIRECTORY_CASE malformed_and_past_end_after_refused_with_correlation");
        let item = DeliveryItem::<BoardDeliveryTypes>::new(
            ItemId::new(durable_steps::Uuid::now_v7())?,
            serde_json::from_value(json!({"kind":"humanUser","installation":SENDER_SERVICE}))?,
            serde_json::from_value(json!({"peer":revoked_peer,"address":{
                "endpoint":{"serviceId":revoked_service,"endpointId":"codex-local"},"sessionId":"revoked-recipient"
            }}))?, None,
            serde_json::from_value(json!({"kind":"message","text":"revoked directory recipient","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}))?,
        )?;
        let mail = BoardConsumerRequest::mail_submit(DeliveryEnvelope::Item(item), None, &revoked_service, revoked_peer)?;
        let submit = request(&mut sender.socket, mail).await?;
        assert!(matches!(submit, OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(board_sync_protocol::MailSubmitAnswer::Submitted(SubmitResult::Refused { reason: DeliveryRefusal::RecipientRevoked })))));
        println!("INSTALLATION_DIRECTORY_CASE listed_revoked_binding_refused_at_submit");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let socket_closed = sender
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    socket_closed?;
    Ok(())
}
