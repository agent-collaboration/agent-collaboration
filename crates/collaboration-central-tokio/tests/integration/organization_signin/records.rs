use super::support::*;
use serde_json::{Value, json};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Three real provisioning records provide independent request identities for exclusive-cursor page assertions."
)]
#[tokio::test]
async fn audit_pages_follow_three_committed_records_once_and_end_after_the_last_id() -> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let first = signed::stamp(A, signed::NOW_SECONDS)?;
    let renewed = signed::renewal(A, &first, signed::NOW_SECONDS, 10)?;
    let renewed_again = signed::renewal(A, &renewed, signed::NOW_SECONDS, 20)?;
    assert_eq!(f.provision(A, &renewed).await?.status, 200);
    assert_eq!(f.provision(A, &renewed_again).await?.status, 200);
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let mut after: Option<String> = None;
    let mut entries = Vec::new();
    for _ in 0..3 {
        let body = json!({"organizationId":A,"after":after,"limit":1});
        let answer = f
            .person(
                signed::RECORDS,
                &body,
                &owner,
                &signed::records_intent(A, after.as_deref(), 1),
                &signed::fresh_request_id(signed::NOW_SECONDS)?,
            )
            .await?;
        assert_eq!(answer.status, 200);
        let answer = answer.value()?;
        assert_eq!(answer.get("result"), Some(&json!("records")));
        let records = answer
            .get("records")
            .and_then(Value::as_array)
            .ok_or("record page missing")?;
        assert_eq!(
            records.len(),
            1,
            "limit=1 returns exactly one remaining record"
        );
        let record = records.first().ok_or("record missing")?.clone();
        after = Some(
            record
                .get("id")
                .and_then(Value::as_str)
                .ok_or("record id missing")?
                .to_owned(),
        );
        entries.push(record);
    }
    let request_ids = entries
        .iter()
        .map(|entry| {
            entry
                .get("requestId")
                .and_then(Value::as_str)
                .ok_or("request id missing")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let renewed_digest = signed::digest(&renewed);
    let renewed_again_digest = signed::digest(&renewed_again);
    assert_eq!(
        request_ids,
        vec![A, renewed_digest.as_str(), renewed_again_digest.as_str()]
    );
    let kinds = entries
        .iter()
        .map(|entry| {
            entry
                .get("kind")
                .and_then(Value::as_str)
                .ok_or("record kind missing")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert_eq!(
        kinds,
        vec!["organizationProvisioned", "stampRenewed", "stampRenewed"]
    );
    let ids = entries
        .iter()
        .map(|entry| entry.get("id").and_then(Value::as_str).ok_or("id missing"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert!(
        ids.windows(2)
            .all(|pair| matches!(pair,[before,after]if before<after)),
        "the exclusive cursor follows strict id order without duplicates"
    );
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        3
    );
    let last = f
        .person(
            signed::RECORDS,
            &json!({"organizationId":A,"after":after,"limit":1}),
            &owner,
            &signed::records_intent(A, after.as_deref(), 1),
            &signed::fresh_request_id(signed::NOW_SECONDS)?,
        )
        .await?;
    assert_eq!(last.status, 200);
    assert_eq!(last.value()?, json!({"result":"records","records":[]}));
    assert_eq!(
        f.snapshot().await?.1.len(),
        3,
        "pagination adds no change record"
    );
    f.close().await
}
