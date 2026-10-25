//! The same real Mailbox retains a held payload beyond terminal-history retention.
#![deny(clippy::too_many_lines)]
use super::{
    mailbox_rule_fixture::{DESTINATION, MailboxPair, ORIGINAL, control},
    relay_transport::{operation, relay_request},
};
use collaboration_central_conformance::ConformanceResult;
use serde_json::{Value, json};

const BODY: &str = "row16 complete held payload across history horizon";

pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let mut pair = MailboxPair::open(base).await?;
    let proof = async {
        let started = chrono::DateTime::from_timestamp(1_893_456_000, 0)
            .ok_or("literal retention origin is invalid")?;
        pin_mailbox_time(base, started).await?;
        let item = pair.item(ORIGINAL, BODY)?;
        let expected = json!({"kind":"delivery","value":{"kind":"fetch","value":{
            "stream":"items","entries":[{"kind":"item","value":{"seq":1,"item":item}}],"through":1}}});
        prepare_held_payload(&mut pair, &expected).await?;
        pin_mailbox_time(base, started + chrono::Duration::days(31)).await?;
        // A fresh CallId reads current custody instead of replaying a prior Fetch answer.
        require_reply(
            operation(&mut pair.recipient.socket, fetch_request()).await?,
            &expected,
            "held payload after the history horizon",
        )?;
        require_reply(
            pair.submit(&item).await?,
            &json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{
                "kind":"submitted","value":{"kind":"replayed","value":{
                    "status":{"kind":"held"},"version":2,"cancel":null}}}}}),
            "same held identity replay",
        )?;
        release_payload(&mut pair).await?;
        println!("HELD_RETENTION_CASE actual_held_payload_survives_31_days_and_exact_receipt_releases_it passed");
        Ok(())
    }
    .await;
    let closed = pair.close().await;
    match (proof, closed) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(proof), Err(cleanup)) => {
            Err(format!("held retention: {proof}; socket cleanup: {cleanup}").into())
        }
    }
}

async fn prepare_held_payload(pair: &mut MailboxPair, expected: &Value) -> ConformanceResult<()> {
    let item = pair.item(ORIGINAL, BODY)?;
    let accepted = pair.submit(&item).await?;
    if accepted.pointer("/value/value/value/kind") != Some(&json!("accepted")) {
        return Err(format!("original item was not accepted: {accepted}").into());
    }
    require_reply(
        operation(&mut pair.recipient.socket, fetch_request()).await?,
        expected,
        "positive current-custody payload",
    )?;
    require_reply(
        operation(
            &mut pair.recipient.socket,
            relay_request(
                "report",
                json!({"report":{"item":ORIGINAL,"reportSeq":1,
                "outcome":{"kind":"held"}}}),
            ),
        )
        .await?,
        &json!({"kind":"delivery","value":{"kind":"report","value":{
            "kind":"applied","value":{"item":ORIGINAL,"acknowledged":1,
                "status":{"kind":"held"},"version":2}}}}),
        "actual Held report",
    )
}

async fn release_payload(pair: &mut MailboxPair) -> ConformanceResult<()> {
    require_reply(
        operation(
            &mut pair.recipient.socket,
            relay_request("received", json!({"stream":"items","through":1})),
        )
        .await?,
        &json!({"kind":"delivery","value":{"kind":"received","value":{
            "stream":"items","releasedThrough":1}}}),
        "exact payload receipt",
    )?;
    let empty = operation(&mut pair.recipient.socket, fetch_request()).await?;
    if empty.pointer("/value/value/entries") != Some(&json!([])) {
        return Err(format!("receipt retained executable payload: {empty}").into());
    }
    Ok(())
}

fn fetch_request() -> Value {
    relay_request(
        "fetch",
        json!({"stream":"items","after":0,
        "maxItems":10,"maxBytes":1_000_000}),
    )
}

async fn pin_mailbox_time(base: &str, now: chrono::DateTime<chrono::Utc>) -> ConformanceResult<()> {
    control(
        base,
        json!({"kind":"mailbox","value":DESTINATION}),
        json!({"kind":"setTime","value":{"now":now}}),
    )
    .await?;
    Ok(())
}

fn require_reply(actual: Value, expected: &Value, context: &str) -> ConformanceResult<()> {
    if &actual != expected {
        return Err(format!("{context}: expected {expected}; observed {actual}").into());
    }
    Ok(())
}
