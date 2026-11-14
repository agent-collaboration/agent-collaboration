//! Signed workspace setup and facts-only expired stamp reads through the private Worker gateway.
use super::support::*;
use central_storage::SqlValue;
use collaboration_central_conformance::signin_fixtures::{self as signed, NOW_SECONDS};
use serde_json::{Value, json};
use std::time::Duration;

const OPERATION: &str = "019f0000-0000-7000-8000-00000000d701";

async fn owned_rows(fixture: &Fixture) -> Vec<Vec<Vec<SqlValue>>> {
    let mut snapshots = Vec::new();
    for sql in [
        "SELECT * FROM organization_identity ORDER BY id",
        "SELECT * FROM organization_change_records ORDER BY id",
        "SELECT * FROM workspaces ORDER BY workspace_id",
        "SELECT * FROM request_log ORDER BY request_id",
        "SELECT * FROM organization_used_requests ORDER BY id",
        "SELECT * FROM actor_identities ORDER BY actor_identity_id",
    ] {
        snapshots.push(fixture.read(sql).await.rows);
    }
    snapshots
}

#[tokio::test]
async fn create_lookup_replay_and_name_uniqueness_are_atomic() {
    // Exercise the actual private gateway with real person proof and current floors.
    let fixture = Fixture::new().await;
    assert_eq!(fixture.provision().await.0, 200);
    fixture.floors().await;
    let owner = fixture.owner();
    let body = signed::create_body(ORG, OPERATION, "Research");
    let intent = signed::create_intent(ORG, OPERATION, "Research");
    let headers = owner
        .headers(
            signed::CREATE,
            &intent,
            &signed::fresh_request_id(NOW_SECONDS).unwrap(),
            NOW_SECONDS,
        )
        .unwrap();
    let (status, reply) = fixture.post_with(signed::CREATE, &body, &headers).await;
    assert_eq!(status, 200, "the signed setup create route must apply");
    assert_eq!(
        reply,
        json!({"result":"applied","workspace":{
            "workspaceId":OPERATION,"name":"Research","description":"","state":"active"
        }})
    );
    let rows = fixture.read("SELECT (SELECT COUNT(*) FROM workspaces),(SELECT COUNT(*) FROM request_log),(SELECT COUNT(*) FROM organization_change_records WHERE kind='workspaceCreated')").await;
    assert_eq!(
        rows.rows,
        vec![vec![
            SqlValue::Integer(1.try_into().unwrap()),
            SqlValue::Integer(1.try_into().unwrap()),
            SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    fixture.close().await;
}

#[tokio::test]
async fn expired_digest_returns_verified_facts_but_create_stays_refused() {
    let fixture = Fixture::new().await;
    assert_eq!(fixture.provision().await.0, 200);
    fixture.floors().await;
    let statement = signed::stamp(ORG, NOW_SECONDS).unwrap();
    let expires = NOW_SECONDS - 60 + signed::LIFETIME_SECONDS;
    let later = expires + 1;
    fixture
        .clock
        .advance(Duration::from_secs(
            u64::try_from(later - NOW_SECONDS).unwrap(),
        ))
        .unwrap();
    fixture.runtime().await;
    let before = owned_rows(&fixture).await;
    let (status, digest) = fixture.post(DIGEST, &json!({"organizationId":ORG})).await;
    assert_eq!(status, 200, "expired stored facts remain readable");
    assert_eq!(
        digest,
        json!({"digest":signed::digest(&statement),"expiresAt":expires,"signingKeyId":"stamp-a"})
    );
    assert!(expires < later);
    let owner = signed::Person::owner(ORG, later).unwrap();
    let body = signed::create_body(ORG, OPERATION, "Expired authority");
    let intent = signed::create_intent(ORG, OPERATION, "Expired authority");
    let headers = owner
        .headers(
            signed::CREATE,
            &intent,
            &signed::fresh_request_id(later).unwrap(),
            later,
        )
        .unwrap();
    assert_eq!(
        fixture.post_with(signed::CREATE, &body, &headers).await.0,
        503,
        "facts read must not authorize a create"
    );
    let records_body = json!({"organizationId":ORG,"after":Value::Null,"limit":100});
    let records_intent = signed::records_intent(ORG, None, 100);
    let headers = owner
        .headers(
            signed::RECORDS,
            &records_intent,
            &signed::fresh_request_id(later).unwrap(),
            later,
        )
        .unwrap();
    assert_eq!(
        fixture
            .post_with(signed::RECORDS, &records_body, &headers)
            .await
            .0,
        503,
        "existing records admission still needs Valid standing"
    );
    assert!(
        owned_rows(&fixture).await == before,
        "facts and refusals changed stamp, change, directory, ledger, proof or actor rows"
    );
    fixture.close().await;
}

#[tokio::test]
async fn not_yet_valid_digest_returns_facts_without_live_authority() {
    let fixture = Fixture::new().await;
    assert_eq!(fixture.provision().await.0, 200);
    fixture.floors().await;
    let before_issuance = NOW_SECONDS - 120;
    fixture
        .clock
        .set_utc(chrono::DateTime::from_timestamp(before_issuance, 0).unwrap());
    fixture.runtime().await;
    let before = owned_rows(&fixture).await;
    let statement = signed::stamp(ORG, NOW_SECONDS).unwrap();
    let (status, facts) = fixture.post(DIGEST, &json!({"organizationId":ORG})).await;
    assert_eq!(
        status, 200,
        "stored facts remain readable before issuance too"
    );
    assert_eq!(
        facts,
        json!({"digest":signed::digest(&statement),
        "expiresAt":NOW_SECONDS-60+signed::LIFETIME_SECONDS,"signingKeyId":"stamp-a"})
    );
    let owner = signed::Person::owner(ORG, before_issuance).unwrap();
    let body = signed::create_body(ORG, OPERATION, "Before issuance");
    let intent = signed::create_intent(ORG, OPERATION, "Before issuance");
    let headers = owner
        .headers(
            signed::CREATE,
            &intent,
            &signed::fresh_request_id(before_issuance).unwrap(),
            before_issuance,
        )
        .unwrap();
    assert_eq!(
        fixture.post_with(signed::CREATE, &body, &headers).await.0,
        503,
        "facts must not authorize a future organization stamp"
    );
    assert!(owned_rows(&fixture).await == before);
    fixture.close().await;
}
