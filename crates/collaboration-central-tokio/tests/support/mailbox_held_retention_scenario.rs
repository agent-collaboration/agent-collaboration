//! Recipient Held custody survives the history horizon until explicit local release.
use super::{
    mail_push_inbox::PushInbox,
    mail_push_transport::{next_push, pushed},
    mailbox_rule_fixture::{MailboxPair, ORIGIN},
    relay_transport::{operation, relay_request},
};
use collaboration_central_conformance::ConformanceResult;
use fleet_protocol::{
    DeliveryItemRef, PeerId, PushResult, RecipientOutcome, ServerCall, Submitter,
};
use serde_json::{Value, json};
const BODY: &str = "row16 complete held payload across history horizon";
fn require(actual: Value, expected: Value) -> ConformanceResult<()> {
    if actual != expected {
        return Err(format!("held retention returned {actual}; expected {expected}").into());
    }
    Ok(())
}
pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let mut pair = MailboxPair::open(base).await?;
    let directory = tempfile::tempdir()?;
    let owner = PeerId::new(pair.recipient.peer)?;
    let inbox = PushInbox::open(
        &directory.path().join("held.db"),
        owner,
        vec![(
            ORIGIN.to_owned().try_into()?,
            PeerId::new(pair.sender.peer)?,
        )],
    )
    .await?;
    let proof=async {
        let item=pair.item(&durable_steps::Uuid::now_v7().to_string(),BODY)?;
        // Align this injected clock to the immutable UUIDv7 creation input.
        // The 31-day assertion must cross that item's actual history horizon.
        let (seconds,nanos)=item.id().as_uuid().get_timestamp().ok_or("item lacks UUIDv7 time")?.to_unix();
        let created_millis=i64::try_from(seconds.checked_mul(1_000).ok_or("item time overflow")?)?+i64::from(nanos/1_000_000);
        inbox.set_time(created_millis);
        if pair.submit(&item).await?.pointer("/value/value/value/kind")!=Some(&json!("accepted")) {return Err("held original not admitted".into());}
        let (frame,ServerCall::Push {submitter,item:offered,..})=next_push(&mut pair.recipient.socket).await? else {return Err("held original not pushed".into());};
        if submitter!=Submitter::Machine(PeerId::new(pair.sender.peer)?)||serde_json::to_vec(&offered)?!=serde_json::to_vec(&item)? {return Err("held Push changed sender or body".into());}
        let receipt=inbox.accept(submitter,offered).await?;
        if !matches!(receipt,PushResult::Accepted {..}) {return Err("held recipient took no actual custody".into());}
        pushed(&mut pair.recipient.socket,frame,receipt).await?;
        let held=inbox.hold_and_report(item.id()).await?;
        if !matches!(held.outcome,RecipientOutcome::Held) {return Err("real SDK did not retain local Held input".into());}
        require(operation(&mut pair.recipient.socket,relay_request("report",json!({"report":held}))).await?,json!({"kind":"delivery","value":{"kind":"report","value":{"kind":"ack","value":{"reportSeq":1}}}}))?;
        let reference=DeliveryItemRef {item:item.id(),digest:item.digest().clone()};
        let expected=serde_json::to_value(&item)?.get("value").cloned().ok_or("held item value absent")?;
        require(serde_json::to_value(inbox.payload(reference.clone()).await?)?,expected.clone())?;
        let bytes=inbox.payload_bytes().await?;
        if bytes<=0||inbox.count().await?!=1 {return Err("held record lacks its actual payload".into());}
        inbox.advance(31*24*60*60*1_000);
        inbox.drive_retention_if_due(item.id()).await?;
        // A fresh public SDK resolution reads current custody, not a cached
        // transport receipt. Item identity and every original byte must survive.
        require(serde_json::to_value(inbox.payload(reference).await?)?,expected)?;
        if inbox.payload_bytes().await?!=bytes||inbox.count().await?!=1 {return Err("history horizon changed Held custody".into());}
        let replay=pair.submit(&item).await?;
        if replay.pointer("/value/value/value/value/status")!=Some(&json!({"kind":"held"})) {return Err(format!("sender forgot its actual Held status: {replay}").into());}
        let final_report=inbox.release_and_report(item.id()).await?;
        if !matches!(final_report.outcome,RecipientOutcome::Uncertain)||final_report.report_seq.value()!=2 {return Err("explicit recipient release lost its terminal report".into());}
        require(operation(&mut pair.recipient.socket,relay_request("report",json!({"report":final_report}))).await?,json!({"kind":"delivery","value":{"kind":"report","value":{"kind":"ack","value":{"reportSeq":2}}}}))?;
        if inbox.payload_bytes().await?!=0||inbox.count().await?!=1 {return Err("final recipient outcome retained body or forgot identity".into());}
        println!("HELD_RETENTION_CASE actual_held_payload_survives_31_days_until_explicit_recipient_release passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    inbox.close()?;
    let closed = pair.close().await;
    proof?;
    closed?;
    Ok(())
}
