//! Lost sender admission evidence cannot decide a cancellation after a real Push.
use super::{
    mail_push_inbox::PushInbox,
    mail_push_transport::{next_push, pushed, recipient_operation},
    mailbox_rule_fixture::{MailboxPair, ORIGIN, control},
    relay_transport::{operation, relay_request},
};
use collaboration_central_conformance::{ConformanceClient, ConformanceResult};
use fleet_protocol::{
    CancelBody, DeliveryCancel, DeliveryEnvelope, FrameId, ItemId, PeerId, PushResult,
    RecipientOutcome, ServerCall, Submitter,
};
use futures_util::SinkExt;
use serde_json::{Value, json};
use std::time::Duration;
async fn query(pair: &mut MailboxPair, item: ItemId) -> ConformanceResult<Value> {
    operation(
        &mut pair.sender.socket,
        relay_request(
            "query",
            json!({"mailbox":pair.recipient.peer,"items":[item]}),
        ),
    )
    .await
}
fn require(actual: Value, expected: Value) -> ConformanceResult<()> {
    if actual != expected {
        return Err(format!("cancel returned {actual}; expected {expected}").into());
    }
    Ok(())
}
pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let mut pair = MailboxPair::open(base).await?;
    let directory = tempfile::tempdir()?;
    let owner = PeerId::new(pair.recipient.peer)?;
    let claim = Submitter::Machine(PeerId::new(pair.sender.peer)?);
    let inbox = PushInbox::open(
        &directory.path().join("cancel.db"),
        owner,
        vec![(
            ORIGIN.to_owned().try_into()?,
            PeerId::new(pair.sender.peer)?,
        )],
    )
    .await?;
    let proof=async {
        let original=pair.item(&durable_steps::Uuid::now_v7().to_string(),"literal original mailbox body")?;
        let frame=FrameId::new(durable_steps::Uuid::now_v7())?;
        let connection=ConformanceClient::new(base)?.active_connection(ORIGIN,PeerId::new(pair.sender.peer)?).await?;
        control(base,json!({"kind":"fleet"}),json!({"kind":"dropNextAnswer","value":{"connection":connection,"frame":frame}})).await?;
        pair.sender.socket.send(reqwest_websocket::Message::Text(json!({"kind":"request","value":{"frame":frame,"body":{"kind":"consumer","value":{"kind":"mailSubmit","value":{"item":original,"originEpoch":null}}}}}).to_string())).await?;
        let observed=control(base,json!({"kind":"fleet"}),json!({"kind":"awaitDroppedAnswer","value":{"connection":connection,"frame":frame}})).await?;
        if observed.get("answer")!=Some(&json!({"kind":"dropped","value":{"connection":connection,"frame":frame}})) {return Err("the exact real sender admission answer was not dropped".into());}
        // Observe the actual outbound frame before cancelling. This establishes
        // possible recipient reach, while no receipt or local decision exists.
        let (mut original_frame,first)=next_push(&mut pair.recipient.socket).await?;
        let original_call=serde_json::to_vec(&first)?;
        let ServerCall::Push {submitter:original_claim,item:offered_original,..}=first else {return Err("original call was not Push".into());};
        if original_claim!=claim||serde_json::to_vec(&offered_original)?!=serde_json::to_vec(&original)?||inbox.count().await?!=0 {return Err("original Push changed sender/body or fabricated recipient custody".into());}
        let cancel=DeliveryEnvelope::Cancel(DeliveryCancel::new(ItemId::new(durable_steps::Uuid::now_v7())?,original.sender().clone(),original.recipient().clone(),None,CancelBody {target:original.id()})?);
        let submitted=pair.submit(&cancel).await?;
        if submitted.pointer("/value/value/value/kind")!=Some(&json!("accepted"))||submitted.pointer("/value/value/value/value/cancel")!=Some(&json!({"kind":"pending"})) {return Err(format!("unanswered original decided its cancel: {submitted}").into());}
        let pending=query(&mut pair,cancel.id()).await?;
        if pending.pointer("/value/value/statuses/0/state/value/cancel")!=Some(&json!({"kind":"pending"}))||pending.pointer("/value/value/statuses/0/state/value/status")!=Some(&json!({"kind":"sent"}))||inbox.count().await?!=0 {return Err(format!("cancel was decided before recipient intake: {pending}").into());}
        let (control_frame,control_claim,offered_control)=tokio::time::timeout(Duration::from_secs(5),async {
            loop {
                let (frame,call)=next_push(&mut pair.recipient.socket).await?;
                if serde_json::to_vec(&call)?==original_call {original_frame=frame;continue;}
                let ServerCall::Push {submitter,item,..}=call else {return Err("cancel was not pushed".into());};
                if submitter!=claim||serde_json::to_vec(&item)?!=serde_json::to_vec(&cancel)? {return Err("cancel Push changed sender/body".into());}
                return Ok::<_,Box<dyn std::error::Error+Send+Sync>>((frame,submitter,item));
            }
        }).await??;
        // Real SDK intake sees the control first. It cannot decide an unknown
        // original; when the exact original follows, the SDK applies prevention.
        let control_receipt=inbox.accept(control_claim,offered_control).await?;
        if !matches!(control_receipt,PushResult::Accepted {..})||inbox.count().await?!=1 {return Err("control took no actual pending custody".into());}
        pushed(&mut pair.recipient.socket,control_frame,control_receipt).await?;
        let original_receipt=inbox.accept(original_claim,offered_original).await?;
        if !matches!(original_receipt,PushResult::Accepted {..})||inbox.count().await?!=2 {return Err("original did not meet its exact pending control in real custody".into());}
        pushed(&mut pair.recipient.socket,original_frame,original_receipt).await?;
        let allowed=[fleet_protocol::DeliveryItemRef {item:original.id(),digest:original.digest().clone()},fleet_protocol::DeliveryItemRef {item:cancel.id(),digest:cancel.digest().clone()}];
        for item in [original.id(),cancel.id()] {
            let report=inbox.report(item).await?;
            if !matches!(report.outcome,RecipientOutcome::Prevented {cancel:actual} if actual==cancel.id()) {return Err("recipient did not decide exact original/control prevention".into());}
            require(recipient_operation(&mut pair.recipient.socket,&inbox,claim,&allowed,relay_request("report",json!({"report":report}))).await?,json!({"kind":"delivery","value":{"kind":"report","value":{"kind":"ack","value":{"reportSeq":1}}}}))?;
        }
        let decided=query(&mut pair,cancel.id()).await?;
        if decided.pointer("/value/value/statuses/0/state/value/cancel")!=Some(&json!({"kind":"prevented"}))||decided.pointer("/value/value/statuses/0/state/value/status")!=Some(&json!({"kind":"cancelled"})) {return Err(format!("recipient decision did not settle the control: {decided}").into());}
        let original_status=query(&mut pair,original.id()).await?;
        // The SDK retains resolved control bodies; the ordinary original's
        // final outcome releases its body while both identities remain.
        let payload_bytes=inbox.payload_bytes_for(original.id()).await?;
        let items=inbox.count().await?;
        if original_status.pointer("/value/value/statuses/0/state/value/status")!=Some(&json!({"kind":"cancelled"}))||payload_bytes!=0||items!=2 {return Err(format!("matched recipient decision did not settle original custody once: {original_status}, payload_bytes={payload_bytes}, items={items}").into());}
        println!("MAILBOX_RULE_CASE lost_custody_answer_keeps_cancel_pending_until_recipient_decides passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    inbox.close()?;
    let closed = pair.close().await;
    proof?;
    closed?;
    Ok(())
}
