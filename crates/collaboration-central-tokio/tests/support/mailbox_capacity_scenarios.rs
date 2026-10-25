//! A full mailbox keeps one per-item cancellation and settlement allowance.
use super::mailbox_rule_fixture::{DESTINATION, MailboxPair, ORIGINAL, control};
use super::relay_transport::{operation, relay_request};
use collaboration_central_conformance::ConformanceResult;
use serde_json::json;

pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let mut pair = MailboxPair::open(base).await?;
    let proof = async {
        control(base, json!({"kind":"mailbox","value":DESTINATION}), json!({"kind":"smallMailboxCapacity"})).await?;
        let original = pair.item(ORIGINAL, "literal original mailbox body")?;
        let rejected = pair.item("019f0000-0000-7000-8000-000000011083", "must never enter custody")?;
        assert_eq!(pair.submit(&original).await?.pointer("/value/value/value/kind"), Some(&json!("accepted")));
        assert_eq!(pair.submit(&rejected).await?, json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{
            "kind":"submitted","value":{"kind":"refused","value":{"reason":{"kind":"mailboxFull"}}}}
        }}));
        let cancel = pair.cancel("019f0000-0000-7000-8000-000000011084")?;
        assert_eq!(pair.submit(&cancel).await?, json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{
            "kind":"submitted","value":{"kind":"accepted","value":{"seq":1,"status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":{"kind":"pending"}}}
        }}}));
        assert_eq!(operation(&mut pair.sender.socket, relay_request("query", json!({"mailbox":pair.recipient.peer,"items":[rejected.id()]}))).await?,
            json!({"kind":"delivery","value":{"kind":"query","value":{"statuses":[{"item":rejected.id(),"state":{"kind":"unknown"}}]}}}));
        assert_eq!(operation(&mut pair.recipient.socket, relay_request("fetch", json!({"stream":"controls","after":0,"maxItems":10,"maxBytes":1_000_000}))).await?,
            json!({"kind":"delivery","value":{"kind":"fetch","value":{"stream":"controls","entries":[{"kind":"item","value":{"seq":1,"item":cancel}}],"through":1}}}));
        assert_eq!(operation(&mut pair.recipient.socket, relay_request("received", json!({"stream":"controls","through":1}))).await?,
            json!({"kind":"delivery","value":{"kind":"received","value":{"stream":"controls","releasedThrough":1}}}));
        assert_eq!(operation(&mut pair.recipient.socket, relay_request("fetch", json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000}))).await?,
            json!({"kind":"delivery","value":{"kind":"fetch","value":{"stream":"items","entries":[{"kind":"item","value":{"seq":1,"item":original}}],"through":1}}}));
        println!("MAILBOX_RULE_CASE full_mailbox_refuses_before_custody_and_reserves_settlement passed");
        Ok(())
    }.await;
    let closed = pair.close().await;
    closed?;
    proof
}
