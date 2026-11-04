//! Exact replay and conflicting bytes retain one real recipient custody row.
use super::{
    mail_push_inbox::PushInbox,
    mail_push_transport::next_push,
    mailbox_rule_fixture::{MailboxPair, ORIGIN},
};
use collaboration_central_conformance::ConformanceResult;
use fleet_sync_protocol::{PeerId, PushResult, ServerCall, Submitter};
use serde_json::{Value, json};
fn require(actual: Value, expected: Value) -> ConformanceResult<()> {
    if actual != expected {
        return Err(format!("identity returned {actual}; expected {expected}").into());
    }
    Ok(())
}
pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let mut pair = MailboxPair::open(base).await?;
    let directory = tempfile::tempdir()?;
    let inbox = PushInbox::open(
        &directory.path().join("identity.db"),
        PeerId::new(pair.recipient.peer)?,
        vec![(
            ORIGIN.to_owned().try_into()?,
            PeerId::new(pair.sender.peer)?,
        )],
    )
    .await?;
    let proof=async {
        let identity=durable_steps::Uuid::now_v7().to_string();
        let original=pair.item(&identity,"literal original mailbox body")?;
        require(pair.submit(&original).await?,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"kind":"submitted","value":{"kind":"accepted","value":{"status":{"kind":"sent"},"version":1,"cancel":null}}}}}))?;
        require(pair.submit(&original).await?,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"kind":"submitted","value":{"kind":"replayed","value":{"status":{"kind":"sent"},"version":1,"cancel":null}}}}}))?;
        let conflict=pair.item(&identity,"literal conflicting mailbox body")?;
        if original.digest()==conflict.digest() {return Err("conflicting body did not change its independent digest".into());}
        require(pair.submit(&conflict).await?,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"kind":"submitted","value":{"kind":"refused","value":{"reason":{"kind":"itemConflict","value":{"existing":original.digest()}}}}}}}))?;
        let (_,ServerCall::Push {submitter,item:offered,..})=next_push(&mut pair.recipient.socket).await? else {return Err("identity did not deliver its original Push".into());};
        if submitter!=Submitter::Machine(PeerId::new(pair.sender.peer)?)||serde_json::to_vec(&offered)?!=serde_json::to_vec(&original)? {return Err("replay/conflict changed the verified sender or original body".into());}
        let reference=fleet_sync_protocol::DeliveryItemRef {item:original.id(),digest:original.digest().clone()};
        let receipt=inbox.accept(submitter,offered).await?;
        if !matches!(receipt,PushResult::Accepted {..}) {return Err("original did not establish real Inbox custody".into());}
        let saved=inbox.payload(reference).await?;
        require(serde_json::to_value(saved)?,serde_json::to_value(&original)?.get("value").cloned().ok_or("original value absent")?)?;
        if inbox.count().await?!=1 {return Err("replay/conflict created more than one original custody row".into());}
        // No receipt is sent to Fleet here: observing a socket write alone
        // must leave the sender's known identity at Sent1.
        require(pair.submit(&original).await?,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"kind":"submitted","value":{"kind":"replayed","value":{"status":{"kind":"sent"},"version":1,"cancel":null}}}}}))?;
        println!("MAILBOX_RULE_CASE replay_and_conflict_keep_one_original_mailbox_item passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    inbox.close()?;
    let closed = pair.close().await;
    proof?;
    closed?;
    Ok(())
}
