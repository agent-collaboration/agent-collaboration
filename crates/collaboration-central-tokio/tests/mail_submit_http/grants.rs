//! Historical authority is exercised on normal sessions and read back from custody.
use super::*;
use board_sync_protocol::{BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest};
use collaboration_central_conformance::{
    ConformanceClient, enroll_admitted_peer, machine_assertion,
};
use collaboration_central_core::{CoreHost, ObjectRuntime};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{AtomicStore, StoreError, Uuid, storage::TransactionScope};
use fleet_sync_protocol::{
    ClientFrame, DatabaseId, DeliveryEnvelope, Epoch, FrameId, ItemId, OperationAnswer,
    OperationRequest, PeerId, ServerCall, ServerFrame, ServerRequest,
};
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore};
use futures_util::SinkExt;
use message_board::ServiceId;
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::{path::Path, sync::Arc};

use super::registry_setup;
const FIRST: &str = "550e8400-e29b-41d4-a716-4466554400c1";
const SECOND: &str = "550e8400-e29b-41d4-a716-4466554400c3";
const RECIPIENT: &str = "550e8400-e29b-41d4-a716-4466554400c2";

pub(super) async fn start(root: &Path, jwks: &JwksFixtureServer) -> ConformanceResult<TokioServer> {
    Ok(TokioServer::start_conformance(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?)
}
pub(super) async fn connect(
    base: &str,
    credential: &str,
    session: &str,
) -> ConformanceResult<WebSocket> {
    let mut socket = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(format!("{base}/v1/session"))
        .bearer_auth(credential)
        .header("Cf-Access-Jwt-Assertion", machine_assertion(credential)?)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    socket
        .send(Message::Text(
            json!({"kind":"hello","value":{
                "sessionId":session,"schemaSupport":{"maxVersion":1}
            }})
            .to_string(),
        ))
        .await?;
    let welcome = scenarios::receive(&mut socket).await?;
    if welcome.get("kind").and_then(Value::as_str) != Some("welcome")
        || welcome.pointer("/value/sessionId").and_then(Value::as_str) != Some(session)
    {
        return Err(format!("normal historical-grant session did not welcome: {welcome}").into());
    }
    Ok(socket)
}
async fn submit(
    socket: &mut WebSocket,
    item: &DeliveryEnvelope<board_sync_protocol::BoardDeliveryTypes>,
    recipient: PeerId,
) -> ConformanceResult<Value> {
    let frame = FrameId::new(Uuid::now_v7())?;
    let item_id = item.id();
    let item = serde_json::from_value(serde_json::to_value(item)?)?;
    let request = BoardConsumerRequest::mail_submit(
        item,
        Some(Epoch::new(1)?),
        &ServiceId::try_from(RECIPIENT.to_owned())?,
        recipient,
    )?;
    socket
        .send(Message::Text(serde_json::to_string(&ClientFrame::<
            BoardConsumerProtocol,
        >::Request {
            frame,
            body: OperationRequest::Consumer(request),
        })?))
        .await?;
    // The committed custody decision can publish status concurrently with the
    // request's answer. Leave that real call unacknowledged: this test proves
    // submission and custody, not the Router's status-application durability.
    let answer = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let answer = scenarios::receive(socket).await?;
            if matches!(serde_json::from_value::<ServerFrame<BoardConsumerProtocol>>(answer.clone())?,
                ServerFrame::Request { body: ServerRequest::Crate(ServerCall::PublishStatus { publication, .. }), .. }
                if publication.item == item_id) {
                continue;
            }
            return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(answer);
        }
    }).await??;
    match serde_json::from_value::<ServerFrame<BoardConsumerProtocol>>(answer.clone())? {
        ServerFrame::Answer {
            frame: actual,
            body: OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(result)),
        } if actual == frame => Ok(serde_json::to_value(result)?),
        _ => Err(format!("normal MailSubmit lost its correlation: {answer}").into()),
    }
}
fn require_mail_answer(answer: Value, expected: Value) -> ConformanceResult<()> {
    if answer != expected {
        return Err(format!("MailSubmit returned {answer}; expected {expected}").into());
    }
    Ok(())
}
async fn custody(root: &Path) -> ConformanceResult<Vec<ItemId>> {
    let objects = ObjectStore::open(root).await?;
    let config = configuration::registry_config()?;
    let object = objects
        .object(
            ObjectKey::new(format!("mailbox-{RECIPIENT}"))?,
            Default::default(),
            Arc::new(CoreHost::for_store(
                registry_setup::SetupIo,
                config.authoritative_receiver,
            )),
        )
        .await?;
    let runtime = ObjectRuntime::mailbox(
        Arc::new(TokioStorageHost::new(object.clone())),
        registry_setup::SetupIo,
        config.authoritative_receiver,
        ServiceId::try_from(RECIPIENT.to_owned())?,
        collaboration_central_core::central_mailbox_limits()?,
    )?;
    runtime.initialize().await?;
    let result = object
        .transaction(|scope| {
            Box::pin(async move {
                let rows = query::<sqlx_turso::Turso>(
                    "SELECT item_id FROM _fleet_sync_mail_items ORDER BY item_id",
                )
                .fetch_all(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                rows.into_iter()
                    .map(|row| {
                        let item: String = row.try_get("item_id").map_err(StoreError::backend)?;
                        ItemId::try_from(item).map_err(StoreError::backend)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
        })
        .await;
    objects.close().await?;
    Ok(result?)
}

#[tokio::test]
async fn genuine_other_peer_grant_refuses_without_custody_and_old_holder_can_submit_after_move()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let proof = async {
        let server = start(root.path(), &jwks).await?;
        let client = ConformanceClient::new(server.base_url())?;
        let peers = async {
            let first = enroll_admitted_peer(&client,FIRST,"019f0000-0000-7000-8000-000000001011","historical-first","019f0000-0000-7000-8000-000000000c21").await?;
            let second = enroll_admitted_peer(&client,SECOND,"019f0000-0000-7000-8000-000000001012","historical-second","019f0000-0000-7000-8000-000000000c22").await?;
            let recipient = enroll_admitted_peer(&client,RECIPIENT,"019f0000-0000-7000-8000-000000001013","historical-recipient","019f0000-0000-7000-8000-000000000c23").await?;
            let peers = (PeerId::new(first.peer)?,PeerId::new(second.peer)?,PeerId::new(recipient.peer)?);
            for peer in [first,second,recipient] { peer.socket.close(reqwest_websocket::CloseCode::Normal,None).await?; }
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(peers)
        }.await;
        server.shutdown().await?;
        let (first, second, recipient) = peers?;
        let database = DatabaseId::new(Uuid::now_v7())?;
        registry_setup::register_grant_history(root.path(),configuration::registry_config()?,database,vec![(Epoch::new(1)?,first),(Epoch::new(2)?,second)]).await?;

        let server = start(root.path(), &jwks).await?;
        let refused = async {
            let mut socket = connect(server.base_url(),"historical-second","019f0000-0000-7000-8000-000000000c24").await?;
            let result = async {
                for cancel in [false,true] {
                    let item = scenarios::project_envelope(database.as_uuid(),recipient,cancel)?;
                    require_mail_answer(submit(&mut socket,&item,recipient).await?,json!({"kind":"notWriter"}))?;
                }
                Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
            }.await;
            let _ = socket.close(reqwest_websocket::CloseCode::Normal,None).await;
            result
        }.await;
        server.shutdown().await?;
        refused?;
        if !custody(root.path()).await?.is_empty() { return Err("other peer's real grant admitted custody".into()); }
        println!("MAIL_SUBMIT_CASE genuine_other_peer_historical_item_and_cancel_refused_without_custody passed");

        let server = start(root.path(), &jwks).await?;
        let accepted = async {
            let mut socket = connect(server.base_url(),"historical-first","019f0000-0000-7000-8000-000000000c25").await?;
            let result = async {
                let item = scenarios::project_envelope(database.as_uuid(),recipient,false)?;
                let id = item.id();
                require_mail_answer(submit(&mut socket,&item,recipient).await?,json!({"kind":"submitted","value":{"kind":"accepted","value":{
                    "seq":1,"status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":null
                }}}))?;
                require_mail_answer(submit(&mut socket,&item,recipient).await?,json!({"kind":"submitted","value":{"kind":"replayed","value":{
                    "status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":null
                }}}))?;
                Ok::<_,Box<dyn std::error::Error+Send+Sync>>(id)
            }.await;
            let _ = socket.close(reqwest_websocket::CloseCode::Normal,None).await;
            result
        }.await;
        server.shutdown().await?;
        if custody(root.path()).await? != vec![accepted?] { return Err("old grant submit did not retain exactly its one item".into()); }
        println!("MAIL_SUBMIT_CASE old_holder_historical_epoch_after_real_move_is_custodied_once passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    jwks.shutdown().await?;
    proof
}
