//! Replay and changed-content refusal are decided by the real mailbox effect.
use super::mailbox_rule_fixture::{MailboxPair, ORIGINAL};
use super::relay_transport::{operation, relay_request};
use collaboration_central_conformance::ConformanceResult;
use serde_json::json;

pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let mut pair = MailboxPair::open(base).await?;
    let proof = async {
        let original = pair.item(ORIGINAL, "literal original mailbox body")?;
        assert_eq!(pair.submit(&original).await?, json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{
            "kind":"submitted","value":{"kind":"accepted","value":{"seq":1,"status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":null}}
        }}}));
        assert_eq!(pair.submit(&original).await?, json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{
            "kind":"submitted","value":{"kind":"replayed","value":{"status":{"kind":"sent","value":{"seq":1}},"version":1,"cancel":null}}
        }}}));
        let conflict = pair.item(ORIGINAL, "literal conflicting mailbox body")?;
        assert_ne!(original.digest(), conflict.digest());
        assert_eq!(pair.submit(&conflict).await?, json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{
            "kind":"submitted","value":{"kind":"refused","value":{"reason":{"kind":"itemConflict","value":{"existing":original.digest()}}}}
        }}}));
        assert_eq!(operation(&mut pair.recipient.socket, relay_request("fetch",
            json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000}))).await?,
            json!({"kind":"delivery","value":{"kind":"fetch","value":{"stream":"items","entries":[{"kind":"item","value":{"seq":1,"item":original}}],"through":1}}}));
        assert_eq!(operation(&mut pair.recipient.socket, relay_request("fetch",
            json!({"stream":"items","after":1,"maxItems":10,"maxBytes":1_000_000}))).await?,
            json!({"kind":"delivery","value":{"kind":"fetch","value":{"stream":"items","entries":[],"through":1}}}));
        println!("MAILBOX_RULE_CASE replay_and_conflict_keep_one_original_mailbox_item passed");
        Ok(())
    }.await;
    let closed = pair.close().await;
    closed?;
    proof
}
