use super::support::*;
use collaboration_central_conformance::signin_fixtures::{self as proof, NOW_SECONDS};
use serde_json::json;
fn records_body() -> serde_json::Value {
    json!({"organizationId":ORG,"after":null,"limit":100})
}
fn records_intent() -> serde_json::Value {
    json!({"v":1,"operation":"ReadChangeRecords","organization":ORG,"target":ORG,"operationId":null,"params":{"after":null,"limit":100}})
}
#[tokio::test]
async fn real_person_proof_floor_permission_and_fresh_request_rules_precede_reads() {
    let f = Fixture::new().await;
    assert_eq!(f.provision().await.0, 200);
    f.floors().await;
    let owner = f.owner();
    let request_id = proof::fresh_request_id(NOW_SECONDS).unwrap();
    let headers = owner
        .headers(RECORDS, &records_intent(), &request_id, NOW_SECONDS)
        .unwrap();
    let result = f.post_with(RECORDS, &records_body(), &headers).await;
    assert_eq!(result.0, 200);
    assert_eq!(result.1["result"], "records");
    assert_eq!(result.1["records"].as_array().unwrap().len(), 1);
    let before = f.counts().await;
    assert_eq!(
        f.post_with(RECORDS, &records_body(), &headers).await,
        (200, json!({"result":"refused","reason":"requestReused"}))
    );
    assert_eq!(f.counts().await, before);
    let tampered_body = json!({"organizationId":ORG,"after":null,"limit":99});
    let fresh = owner
        .headers(
            RECORDS,
            &records_intent(),
            &proof::fresh_request_id(NOW_SECONDS).unwrap(),
            NOW_SECONDS,
        )
        .unwrap();
    assert_eq!(
        f.post_with(RECORDS, &tampered_body, &fresh).await,
        (401, serde_json::Value::Null)
    );
    assert_eq!(f.counts().await, before);
    let mut below = f.owner();
    below.organization_revision = 2;
    assert_eq!(
        f.records(&below).await,
        (200, json!({"result":"refused","reason":"refreshRequired"}))
    );
    let mut ahead = f.owner();
    ahead.organization_revision = 4;
    assert_eq!(
        f.records(&ahead).await,
        (
            200,
            json!({"result":"refused","reason":"authorizationSyncRequired"})
        )
    );
    let mut denied = f.owner();
    denied.permissions.clear();
    assert_eq!(
        f.records(&denied).await,
        (200, json!({"result":"refused","reason":"notPermitted"}))
    );
    assert_eq!(f.counts().await, before);
    let unknown = "019f0000-0000-7000-8000-00000000d298";
    let body = json!({"organizationId":ORG,"workspaceId":unknown});
    let intent = json!({"v":1,"operation":"WorkspaceLookup","organization":ORG,"target":unknown,"operationId":null,"params":{}});
    let headers = owner
        .headers(
            LOOKUP,
            &intent,
            &proof::fresh_request_id(NOW_SECONDS).unwrap(),
            NOW_SECONDS,
        )
        .unwrap();
    assert_eq!(
        f.post_with(LOOKUP, &body, &headers).await,
        (200, json!({"result":"found","workspace":null}))
    );
    assert_eq!(
        f.post_with(RECORDS, &records_body(), &[]).await,
        (401, serde_json::Value::Null)
    );
    f.close().await;
}
#[tokio::test]
async fn expired_and_currently_revoked_person_requests_reserve_nothing() {
    let f = Fixture::new().await;
    assert_eq!(f.provision().await.0, 200);
    f.floors().await;
    let owner = f.owner();
    let headers = owner
        .headers_with(
            RECORDS,
            &records_intent(),
            &proof::fresh_request_id(NOW_SECONDS).unwrap(),
            NOW_SECONDS,
            &|claims| {
                claims["exp"] = json!(NOW_SECONDS - 1);
            },
        )
        .unwrap();
    let before = f.counts().await;
    assert_eq!(
        f.post_with(RECORDS, &records_body(), &headers).await,
        (401, serde_json::Value::Null)
    );
    assert_eq!(f.counts().await, before);
    assert_eq!(
        f.post(REVISIONS, &person_revision(5, "inactive")).await.0,
        200
    );
    assert_eq!(
        f.records(&owner).await,
        (200, json!({"result":"refused","reason":"notPermitted"}))
    );
    assert_eq!(
        f.counts().await.rows[0][2],
        central_storage::SqlValue::Integer(0.try_into().unwrap())
    );
    f.close().await;
}
