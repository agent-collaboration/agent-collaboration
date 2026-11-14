//! Audit cursor assertions use three real provisioning and renewal effects.
use super::support::{Fixture, ORG, PROVISION, RECORDS};
use collaboration_central_conformance::signin_fixtures::{self as signed, NOW_SECONDS};
use serde_json::{Value, json};

#[tokio::test]
async fn audit_pages_follow_three_committed_records_once_and_end_after_the_last_id() {
    let f = Fixture::new().await;
    assert_eq!(f.provision().await.0, 200);
    f.floors().await;
    let first = signed::stamp(ORG, NOW_SECONDS).unwrap();
    let renewed = signed::renewal(ORG, &first, NOW_SECONDS, 10).unwrap();
    let renewed_again = signed::renewal(ORG, &renewed, NOW_SECONDS, 20).unwrap();
    for statement in [&renewed, &renewed_again] {
        assert_eq!(
            f.post(
                PROVISION,
                &json!({"organizationId":ORG,"organizationStatement":statement})
            )
            .await
            .0,
            200
        );
    }
    let owner = f.owner();
    let mut after: Option<String> = None;
    let mut entries = Vec::new();
    for _ in 0..3 {
        let body = json!({"organizationId":ORG,"after":after,"limit":1});
        let headers = owner
            .headers(
                RECORDS,
                &signed::records_intent(ORG, after.as_deref(), 1),
                &signed::fresh_request_id(NOW_SECONDS).unwrap(),
                NOW_SECONDS,
            )
            .unwrap();
        let answer = f.post_with(RECORDS, &body, &headers).await;
        assert_eq!(answer.0, 200);
        assert_eq!(answer.1.get("result"), Some(&json!("records")));
        let records = answer.1.get("records").and_then(Value::as_array).unwrap();
        assert_eq!(
            records.len(),
            1,
            "limit=1 returns exactly one remaining record"
        );
        let record = records.first().unwrap().clone();
        after = Some(record.get("id").and_then(Value::as_str).unwrap().to_owned());
        entries.push(record);
    }
    let request_ids = entries
        .iter()
        .map(|entry| entry.get("requestId").and_then(Value::as_str).unwrap())
        .collect::<Vec<_>>();
    let renewed_digest = signed::digest(&renewed);
    let renewed_again_digest = signed::digest(&renewed_again);
    assert_eq!(
        request_ids,
        vec![ORG, renewed_digest.as_str(), renewed_again_digest.as_str()]
    );
    let kinds = entries
        .iter()
        .map(|entry| entry.get("kind").and_then(Value::as_str).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec!["organizationProvisioned", "stampRenewed", "stampRenewed"]
    );
    let ids = entries
        .iter()
        .map(|entry| entry.get("id").and_then(Value::as_str).unwrap())
        .collect::<Vec<_>>();
    assert!(
        ids.windows(2)
            .all(|pair| matches!(pair,[before,after]if before<after)),
        "the exclusive cursor follows strict id order without duplicates"
    );
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        3
    );
    let headers = owner
        .headers(
            RECORDS,
            &signed::records_intent(ORG, after.as_deref(), 1),
            &signed::fresh_request_id(NOW_SECONDS).unwrap(),
            NOW_SECONDS,
        )
        .unwrap();
    assert_eq!(
        f.post_with(
            RECORDS,
            &json!({"organizationId":ORG,"after":after,"limit":1}),
            &headers
        )
        .await,
        (200, json!({"result":"records","records":[]}))
    );
    assert_eq!(
        f.read("SELECT id FROM organization_change_records")
            .await
            .rows
            .len(),
        3,
        "pagination adds no change record"
    );
    f.close().await;
}
