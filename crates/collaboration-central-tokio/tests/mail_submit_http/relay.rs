//! Authenticated Push receipts and reports retain one immutable item.
pub(super) use super::mail_push_transport::{next_push, pushed};
pub(super) use super::relay_transport::{operation, relay_request};
use super::*;
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_conformance::{ConformanceClient, enroll_admitted_peer};
use durable_steps::Uuid;
use fleet_sync_delivery::DeliveryLimits;
use fleet_sync_protocol::{
    DeliveryEnvelope, DeliveryItem, DeliveryRefusal, ItemId, PeerId, PushResult, ServerCall,
    Submitter,
};
use reqwest_websocket::WebSocket;
use serde_json::{Value, json};
pub(super) fn require(actual: Value, expected: Value) -> ConformanceResult<()> {
    if actual != expected {
        return Err(format!("mail relay returned {actual}; expected {expected}").into());
    }
    Ok(())
}
pub(super) fn submitted(item: &DeliveryEnvelope<BoardDeliveryTypes>) -> Value {
    json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"item":item,"originEpoch":null}}})
}
#[tokio::test]
async fn normal_recipient_session_pushes_receipts_and_reports_original_mail()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let inbox_root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = grants::start(root.path(), &jwks).await?;
    let proof=async {
        const ORIGIN:&str="019f0000-0000-7000-8000-000000004017";
        const DESTINATION:&str="019f0000-0000-7000-8000-000000004018";
        let client=ConformanceClient::new(server.base_url())?;
        let mut sender=enroll_admitted_peer(&client,ORIGIN,"019f0000-0000-7000-8000-000000001017","relay-origin","019f0000-0000-7000-8000-000000000e51").await?;
        let mut recipient=enroll_admitted_peer(&client,DESTINATION,"019f0000-0000-7000-8000-000000001018","relay-recipient","019f0000-0000-7000-8000-000000000e52").await?;
        let owner=PeerId::new(recipient.peer)?;let submitter=Submitter::Machine(PeerId::new(sender.peer)?);
        let inbox=mail_push_inbox::PushInbox::open(&inbox_root.path().join("inbox.db"),owner,vec![(message_board::ServiceId::try_from(ORIGIN.to_owned())?,PeerId::new(sender.peer)?)]).await?;
        let proof=async {
            let item=DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(ItemId::new(Uuid::now_v7())?,
                serde_json::from_value(json!({"kind":"humanUser","installation":ORIGIN}))?,
                serde_json::from_value(json!({"peer":owner,"address":{"endpoint":{"serviceId":DESTINATION,"endpointId":"codex-local"},"sessionId":"recipient"}}))?,None,
                serde_json::from_value(json!({"kind":"message","text":"normal recipient custody","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}))?)?);
            require(operation(&mut sender.socket,submitted(&item)).await?,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"kind":"submitted","value":{"kind":"accepted","value":{"status":{"kind":"sent"},"version":1,"cancel":null}}}}}))?;
            let (frame,first)=next_push(&mut recipient.socket).await?;let first_bytes=serde_json::to_vec(&first)?;
            let ServerCall::Push {submitter:actual,item:offered,..}=first else {return Err("expected Push".into());};
            require(serde_json::to_value(&offered)?,serde_json::to_value(&item)?)?;
            if actual!=submitter {return Err("Push changed the independently enrolled sender".into());}
            let no_room=DeliveryLimits::new::<BoardDeliveryTypes>(1_000_000,100,1_000_000,100,0,fleet_sync_delivery::minimum_settlement_allowance::<BoardDeliveryTypes>()?)?;
            let refused=inbox.accept_with_limits(actual,offered,no_room).await?;
            if !matches!(refused,PushResult::Refused {reason:DeliveryRefusal::MailboxFull})||inbox.count().await?!=0 {return Err("full real Inbox took custody".into());}
            pushed(&mut recipient.socket,frame,refused).await?;
            let (frame,retry)=next_push(&mut recipient.socket).await?;
            if serde_json::to_vec(&retry)?!=first_bytes {return Err("capacity retry changed its CallId/sender/body".into());}
            let ServerCall::Push {submitter:actual,item:offered,..}=retry else {return Err("expected retry Push".into());};
            let receipt=inbox.accept(actual,offered).await?;
            if !matches!(receipt,PushResult::Accepted {..}) {return Err("real SDK did not accept fresh custody".into());}
            pushed(&mut recipient.socket,frame,receipt).await?;
            wait_for_receipt(&mut sender.socket, &item, owner).await?;
            let reference=fleet_sync_protocol::DeliveryItemRef {item:item.id(),digest:item.digest().clone()};
            let saved=inbox.payload(reference).await?;
            require(serde_json::to_value(saved)?,serde_json::to_value(&item)?.get("value").cloned().ok_or("item payload absent")?)?;
            if inbox.count().await?!=1 || inbox.used_bytes().await?==0 {return Err("Push retry duplicated custody or lost its quota accounting".into());}
            let held=inbox.hold_and_report(item.id()).await?;
            require(operation(&mut recipient.socket,relay_request("report",json!({"report":held}))).await?,json!({"kind":"delivery","value":{"kind":"report","value":{"kind":"ack","value":{"reportSeq":1}}}}))?;
            require_known(&mut sender.socket, &item, owner, "held", 3).await?;
            // The authenticated recipient supplies its later local uncertainty;
            // the real Fleet validates sequence and advances its stored status.
            require(operation(&mut recipient.socket,relay_request("report",json!({"report":{"item":item.id(),"reportSeq":2,"outcome":{"kind":"uncertain"}}}))).await?,json!({"kind":"delivery","value":{"kind":"report","value":{"kind":"ack","value":{"reportSeq":2}}}}))?;
            require_known(&mut sender.socket, &item, owner, "uncertain", 4).await?;
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        let _=sender.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        let _=recipient.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        inbox.close()?;proof
    }.await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    proof?;
    stopped?;
    keys?;
    Ok(())
}

async fn require_known(
    socket: &mut WebSocket,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
    owner: PeerId,
    status: &str,
    version: u64,
) -> ConformanceResult<()> {
    require(
        operation(
            socket,
            relay_request("query", json!({"mailbox":owner,"items":[item.id()]})),
        )
        .await?,
        json!({"kind":"delivery","value":{"kind":"query","value":{"statuses":[{"item":item.id(),"state":{"kind":"known","value":{"digest":item.digest(),"status":{"kind":status},"version":version,"cancel":null}}}]}}}),
    )
}

pub(super) async fn wait_for_receipt(
    socket: &mut WebSocket,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
    owner: PeerId,
) -> ConformanceResult<()> {
    // A socket answer can race a later report. Observe its committed custody
    // first so this scenario deliberately pins both numbered transitions.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let actual = operation(socket,relay_request("query",json!({"mailbox":owner,"items":[item.id()]}))).await?;
            let expected = json!({"kind":"delivery","value":{"kind":"query","value":{"statuses":[{"item":item.id(),"state":{"kind":"known","value":{"digest":item.digest(),"status":{"kind":"atRecipient"},"version":2,"cancel":null}}}]}}});
            if actual == expected { return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(()); }
            require(actual,json!({"kind":"delivery","value":{"kind":"query","value":{"statuses":[{"item":item.id(),"state":{"kind":"known","value":{"digest":item.digest(),"status":{"kind":"sent"},"version":1,"cancel":null}}}]}}}))?;
            tokio::task::yield_now().await;
        }
    }).await?
}
