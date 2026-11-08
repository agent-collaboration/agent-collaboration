//! A sender retires only its own stored prefix through normal session frames.
use super::{
    grants::{connect, start},
    relay::{next_push, operation, pushed, relay_request, require, submitted},
    *,
};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_conformance::{ConformanceClient, enroll_admitted_peer};
use durable_steps::Uuid;
use fleet_protocol::{
    DeliveryEnvelope, DeliveryItem, ItemId, PeerId, PushResult, ServerCall, ServerFrame,
    ServerRequest, Submitter,
};
use reqwest_websocket::{CloseCode, WebSocket};
use serde_json::json;

const SENDER: &str = "550e8400-e29b-41d4-a716-4466554400fa";
const RECIPIENT: &str = "550e8400-e29b-41d4-a716-4466554400fb";
const SENDER_CREDENTIAL: &str = "status-ack-origin";

async fn submit_and_wait_for_stored_status(
    socket: &mut WebSocket,
    recipient: &mut WebSocket,
    inbox: &mail_push_inbox::PushInbox,
    submitter: Submitter,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> ConformanceResult<()> {
    require(
        operation(socket, submitted(item)).await?,
        json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"kind":"submitted","value":{"kind":"accepted","value":{"status":{"kind":"sent"},"version":1,"cancel":null}}}}}),
    )?;
    let (
        frame,
        ServerCall::Push {
            submitter: actual,
            item: offered,
            ..
        },
    ) = next_push(recipient).await?
    else {
        return Err("expected current Push custody request".into());
    };
    if actual != submitter || serde_json::to_vec(&offered)? != serde_json::to_vec(item)? {
        return Err("status setup Push changed sender or payload".into());
    }
    let receipt = inbox.accept(actual, offered).await?;
    if !matches!(receipt, PushResult::Accepted { .. }) || inbox.count().await? != 1 {
        return Err("status setup did not commit real recipient custody".into());
    }
    pushed(recipient, frame, receipt).await?;
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            let received=scenarios::receive(socket).await?;
            match serde_json::from_value::<ServerFrame<BoardConsumerProtocol>>(received)? {
                ServerFrame::Request {body:ServerRequest::Crate(ServerCall::PublishStatus {publication,..}),..} if publication.item==item.id()=>{
                    require(serde_json::to_value(publication)?,json!({"item":item.id(),"status":{"kind":"atRecipient"},"version":2,"cancel":null}))?;
                    // A live publication is sent only after the stream's append
                    // committed. It is a readiness signal, not a Router ack.
                    return Ok(());
                }
                ServerFrame::Notice {..} => {},
                _=>return Err("expected this submit's correlated answer or stored status publication".into()),
            }
        }
    }).await?
}

#[tokio::test]
async fn normal_status_ack_refuses_future_prefix_and_retires_only_its_own_stream_after_restart()
-> ConformanceResult<()> {
    // A missing own-stream route closes the session; a no-op acknowledgement
    // leaves the literal AtRecipient event fetchable. A heap-only floor loses the
    // retirement after the whole server restarts. Every status is real custody.
    let root = tempfile::tempdir()?;
    let inbox_root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let proof=async {
        let first=start(root.path(),&jwks).await?;
        let proof=async {
            let client=ConformanceClient::new(first.base_url())?;
            let mut sender=enroll_admitted_peer(&client,SENDER,"019f0000-0000-7000-8000-00000000101b",SENDER_CREDENTIAL,"019f0000-0000-7000-8000-000000000fa1").await?;
            let mut recipient=enroll_admitted_peer(&client,RECIPIENT,"019f0000-0000-7000-8000-00000000101c","status-ack-recipient","019f0000-0000-7000-8000-000000000fb1").await?;
            let inbox=mail_push_inbox::PushInbox::open(&inbox_root.path().join("status-inbox.db"),PeerId::new(recipient.peer)?,vec![(SENDER.to_owned().try_into()?,PeerId::new(sender.peer)?)]).await?;
            let proof=async {
                let id=ItemId::new(Uuid::now_v7())?;
                let item=DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(id,
                    serde_json::from_value(json!({"kind":"humanUser","installation":SENDER}))?,
                    serde_json::from_value(json!({"peer":PeerId::new(recipient.peer)?,"address":{"endpoint":{"serviceId":RECIPIENT,"endpointId":"codex-local"},"sessionId":"status recipient"}}))?,None,
                    serde_json::from_value(json!({"kind":"message","text":"retire the stored status prefix","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}))?,
                )?);
                submit_and_wait_for_stored_status(&mut sender.socket,&mut recipient.socket,&inbox,Submitter::Machine(PeerId::new(sender.peer)?),&item).await?;
                let fetch=relay_request("statusFetch",json!({"after":0,"limit":10}));
                let stored=json!({"kind":"delivery","value":{"kind":"statusFetch","value":{"events":[{"cursor":1,"item":id,"status":{"kind":"atRecipient"},"version":2,"cancel":null}],"through":1}}});
                require(operation(&mut sender.socket,fetch.clone()).await?,stored.clone())?;
                require(operation(&mut recipient.socket,relay_request("statusAck",json!({"through":1}))).await?,
                    json!({"kind":"delivery","value":{"kind":"statusAck","value":{"kind":"refused","value":{"head":0}}}}))?;
                require(operation(&mut sender.socket,fetch.clone()).await?,stored.clone())?;
                require(operation(&mut sender.socket,relay_request("statusAck",json!({"through":2}))).await?,
                    json!({"kind":"delivery","value":{"kind":"statusAck","value":{"kind":"refused","value":{"head":1}}}}))?;
                require(operation(&mut sender.socket,fetch.clone()).await?,stored)?;
                let ack=relay_request("statusAck",json!({"through":1}));
                let acknowledged=json!({"kind":"delivery","value":{"kind":"statusAck","value":{"kind":"acknowledged","value":{"floor":1}}}});
                require(operation(&mut sender.socket,ack.clone()).await?,acknowledged.clone())?;
                require(operation(&mut sender.socket,ack.clone()).await?,acknowledged.clone())?;
                require(operation(&mut sender.socket,fetch.clone()).await?,json!({"kind":"delivery","value":{"kind":"statusFetch","value":{"events":[],"through":0}}}))?;
                Ok::<_,Box<dyn std::error::Error+Send+Sync>>((fetch,ack,acknowledged))
            }.await;
            let _=sender.socket.close(CloseCode::Normal,None).await;
            let _=recipient.socket.close(CloseCode::Normal,None).await;
            inbox.close()?;
            proof
        }.await;
        first.shutdown().await?;
        let (fetch,ack,acknowledged)=proof?;
        let second=start(root.path(),&jwks).await?;
        let proof=async {
            let mut socket=connect(second.base_url(),SENDER_CREDENTIAL,"019f0000-0000-7000-8000-000000000fa2").await?;
            let proof=async {
                require(operation(&mut socket,fetch).await?,json!({"kind":"delivery","value":{"kind":"statusFetch","value":{"events":[],"through":0}}}))?;
                require(operation(&mut socket,ack).await?,acknowledged)?;
                Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
            }.await;
            let _=socket.close(CloseCode::Normal,None).await;
            proof
        }.await;
        second.shutdown().await?;
        proof
    }.await;
    jwks.shutdown().await?;
    proof
}
