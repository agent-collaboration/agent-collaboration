//! A full real recipient Inbox reserves room for cancelling its original item.
use super::{
    mail_push_inbox::PushInbox,
    mail_push_transport::{next_push, pushed},
    mailbox_rule_fixture::{MailboxPair, ORIGIN, control},
    relay_transport::{operation, relay_request},
};
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_conformance::ConformanceResult;
use fleet_sync_delivery::DeliveryLimits;
use fleet_sync_protocol::{
    CancelBody, DeliveryCancel, DeliveryEnvelope, DeliveryRefusal, ItemId, PeerId, PushResult,
    ServerCall, Submitter,
};
use serde_json::{Value, json};
const NOW: i64 = 1_792_281_600_000;
fn require(actual: Value, expected: Value) -> ConformanceResult<()> {
    if actual != expected {
        return Err(format!("capacity returned {actual}; expected {expected}").into());
    }
    Ok(())
}
pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let mut pair = MailboxPair::open(base).await?;
    let directory = tempfile::tempdir()?;
    let owner = PeerId::new(pair.recipient.peer)?;
    let claim = Submitter::Machine(PeerId::new(pair.sender.peer)?);
    let inbox = PushInbox::open(
        &directory.path().join("capacity.db"),
        owner,
        vec![(
            ORIGIN.to_owned().try_into()?,
            PeerId::new(pair.sender.peer)?,
        )],
    )
    .await?;
    let proof=async {
        control(base,json!({"kind":"fleet"}),json!({"kind":"setTime","value":{"now":chrono::DateTime::from_timestamp_millis(NOW).ok_or("invalid capacity clock")?}})).await?;
        let original=pair.item(&durable_steps::Uuid::now_v7().to_string(),"literal original mailbox body")?;
        let rejected=pair.item(&durable_steps::Uuid::now_v7().to_string(),"must never enter full recipient custody")?;
        if pair.submit(&original).await?.pointer("/value/value/value/kind")!=Some(&json!("accepted")) {return Err("original not admitted to Fleet".into());}
        let (frame,ServerCall::Push {submitter,item:offered,..})=next_push(&mut pair.recipient.socket).await? else {return Err("original not pushed".into());};
        if submitter!=claim||serde_json::to_vec(&offered)?!=serde_json::to_vec(&original)? {return Err("original Push changed sender/body".into());}
        let receipt=inbox.accept(submitter,offered).await?;
        if !matches!(receipt,PushResult::Accepted {..}) {return Err("real recipient did not take original custody".into());}
        pushed(&mut pair.recipient.socket,frame,receipt).await?;
        let used=inbox.used_bytes().await?;
        if used==0||inbox.count().await?!=1 {return Err("original custody has no quota charge".into());}
        let full=DeliveryLimits::new::<BoardDeliveryTypes>(1_000_000,100,1_000_000,100,used,fleet_sync_delivery::minimum_settlement_allowance::<BoardDeliveryTypes>()?)?;
        if pair.submit(&rejected).await?.pointer("/value/value/value/kind")!=Some(&json!("accepted")) {return Err("Fleet refused before recipient capacity could be exercised".into());}
        let (frame,ServerCall::Push {submitter,item:offered,..})=next_push(&mut pair.recipient.socket).await? else {return Err("new item not pushed to full Inbox".into());};
        if submitter!=claim||serde_json::to_vec(&offered)?!=serde_json::to_vec(&rejected)? {return Err("capacity request changed sender/body".into());}
        let no_room=inbox.accept_with_limits(submitter,offered,full).await?;
        if !matches!(no_room,PushResult::Refused {reason:DeliveryRefusal::MailboxFull})||inbox.count().await?!=1||inbox.used_bytes().await?!=used {return Err("full Inbox admitted payload or changed its reserve".into());}
        pushed(&mut pair.recipient.socket,frame,no_room).await?;
        // Rejection is at B before custody. Fleet truth remains Sent; it may
        // retry later after room is freed, but no false recipient receipt exists.
        require(operation(&mut pair.sender.socket,relay_request("query",json!({"mailbox":owner,"items":[rejected.id()]}))).await?,json!({"kind":"delivery","value":{"kind":"query","value":{"statuses":[{"item":rejected.id(),"state":{"kind":"known","value":{"digest":rejected.digest(),"status":{"kind":"sent"},"version":1,"cancel":null}}}]}}}))?;
        let cancel=DeliveryEnvelope::Cancel(DeliveryCancel::new(ItemId::new(durable_steps::Uuid::now_v7())?,original.sender().clone(),original.recipient().clone(),None,CancelBody {target:original.id()})?);
        let accepted=pair.submit(&cancel).await?;
        if accepted.pointer("/value/value/value/kind")!=Some(&json!("accepted"))||accepted.pointer("/value/value/value/value/cancel")!=Some(&json!({"kind":"pending"})) {return Err(format!("full recipient lost cancellation admission: {accepted}").into());}
        let (frame,ServerCall::Push {submitter,item:offered,..})=next_push(&mut pair.recipient.socket).await? else {return Err("reserved cancellation not pushed".into());};
        if submitter!=claim||serde_json::to_vec(&offered)?!=serde_json::to_vec(&cancel)? {return Err("reserved cancellation changed sender/body".into());}
        let receipt=inbox.accept_with_limits(submitter,offered,full).await?;
        if !matches!(receipt,PushResult::Accepted {..})||inbox.count().await?!=2 {return Err("full Inbox failed to use its prepaid cancellation reserve".into());}
        pushed(&mut pair.recipient.socket,frame,receipt).await?;
        for item in [original.id(),cancel.id()] {
            let report=inbox.report(item).await?;
            if !matches!(report.outcome,fleet_sync_protocol::RecipientOutcome::Prevented {cancel:actual} if actual==cancel.id()) {return Err("real cancellation lost its exact prevention outcome".into());}
            require(operation(&mut pair.recipient.socket,relay_request("report",json!({"report":report}))).await?,json!({"kind":"delivery","value":{"kind":"report","value":{"kind":"ack","value":{"reportSeq":1}}}}))?;
        }
        println!("MAILBOX_RULE_CASE full_mailbox_refuses_before_custody_and_reserves_settlement passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    inbox.close()?;
    let closed = pair.close().await;
    proof?;
    closed?;
    Ok(())
}
