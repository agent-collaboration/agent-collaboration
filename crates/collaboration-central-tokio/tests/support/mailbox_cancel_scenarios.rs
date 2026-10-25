//! A real lost custody answer cannot decide a cancellation for the recipient.
use super::mailbox_rule_fixture::{MailboxPair, ORIGIN, ORIGINAL, control};
use super::relay_transport::{operation, relay_request};
use collaboration_central_conformance::ConformanceResult;
use futures_util::SinkExt;
use serde_json::{Value, json};

async fn query(pair: &mut MailboxPair, item: &str) -> ConformanceResult<Value> {
    operation(
        &mut pair.sender.socket,
        relay_request(
            "query",
            json!({"mailbox":pair.recipient.peer,"items":[item]}),
        ),
    )
    .await
}
pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let mut pair = MailboxPair::open(base).await?;
    let proof = async {
        let original = pair.item(ORIGINAL, "literal original mailbox body")?;
        let frame = "019f0000-0000-7000-8000-000000031081";
        let connection = json!({"kind":"connection","value":ORIGIN});
        control(base, connection.clone(), json!({"kind":"dropNextAnswer","value":{"frame":frame}})).await?;
        pair.sender.socket.send(reqwest_websocket::Message::Text(json!({"kind":"request","value":{
            "frame":frame,"body":{"kind":"consumer","value":{"kind":"mailSubmit","value":{"item":original,"originEpoch":null}}}
        }}).to_string())).await?;
        let observed = control(base, connection, json!({"kind":"awaitDroppedAnswer","value":{"frame":frame}})).await?;
        assert_eq!(observed.get("answer"), Some(&json!({"kind":"dropped","value":{"frame":frame}})));
        let cancel_id = "019f0000-0000-7000-8000-000000011084";
        let cancel = pair.cancel(cancel_id)?;
        assert_eq!(pair.submit(&cancel).await?.pointer("/value/value/value/value/cancel"), Some(&json!({"kind":"pending"})));
        let pending = query(&mut pair, cancel_id).await?;
        assert_eq!(pending.pointer("/value/value/statuses/0/state/value/cancel"), Some(&json!({"kind":"pending"})));
        assert_eq!(pending.pointer("/value/value/statuses/0/state/value/status"), Some(&json!({"kind":"sent","value":{"seq":1}})));
        assert_eq!(operation(&mut pair.recipient.socket, relay_request("fetch", json!({"stream":"controls","after":0,"maxItems":10,"maxBytes":1_000_000}))).await?,
            json!({"kind":"delivery","value":{"kind":"fetch","value":{"stream":"controls","entries":[{"kind":"item","value":{"seq":1,"item":cancel}}],"through":1}}}));
        for item in [ORIGINAL, cancel_id] {
            let report = operation(&mut pair.recipient.socket, relay_request("report", json!({"report":{"item":item,"reportSeq":1,"outcome":{"kind":"prevented","value":{"cancel":cancel_id}}}}))).await?;
            assert_eq!(report.pointer("/value/value/value/status"), Some(&json!({"kind":"cancelled"})));
        }
        let decided = query(&mut pair, cancel_id).await?;
        assert_eq!(decided.pointer("/value/value/statuses/0/state/value/cancel"), Some(&json!({"kind":"prevented"})));
        assert_eq!(query(&mut pair, ORIGINAL).await?.pointer("/value/value/statuses/0/state/value/status"), Some(&json!({"kind":"cancelled"})));
        println!("MAILBOX_RULE_CASE lost_custody_answer_keeps_cancel_pending_until_recipient_decides passed");
        Ok(())
    }.await;
    let closed = pair.close().await;
    closed?;
    proof
}
