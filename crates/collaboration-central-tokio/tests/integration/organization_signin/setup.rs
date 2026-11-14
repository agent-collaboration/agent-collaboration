//! Owner-approved workspace setup and facts-only expired stamp reads.
use super::support::*;
use serde_json::{Value, json};

const OPERATION: &str = "019f0000-0000-7000-8000-00000000d701";

mod authorization;
mod deadlines;
mod directory;
mod recovery;
mod support;

async fn admission_rows(fixture: &Fixture) -> Result<Vec<Vec<Vec<central_storage::SqlValue>>>> {
    let mut snapshots = Vec::new();
    for sql in [
        "SELECT * FROM workspaces ORDER BY workspace_id",
        "SELECT * FROM request_log ORDER BY request_id",
        "SELECT * FROM organization_used_requests ORDER BY id",
        "SELECT * FROM actor_identities ORDER BY actor_identity_id",
    ] {
        snapshots.push(fixture.rows(A, sql, vec![]).await?);
    }
    Ok(snapshots)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Real signed HTTP and owning-store assertions must fail while fixture errors propagate."
)]
#[tokio::test]
async fn create_lookup_replay_and_name_uniqueness_are_atomic() -> Result {
    // Arrange: real sign-in keys, private Unix HTTP route and current floors.
    let fixture = Fixture::new().await?;
    fixture.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let body = signed::create_body(A, OPERATION, "Research");
    let intent = signed::create_intent(A, OPERATION, "Research");

    // Act: the signed setup request must reach the actual mounted create route.
    let first_proof = signed::fresh_request_id(signed::NOW_SECONDS)?;
    let reply = fixture
        .person(signed::CREATE, &body, &owner, &intent, &first_proof)
        .await?;

    // Assert: literal wire contract, with operation identity owning the workspace.
    assert_eq!(
        reply.status, 200,
        "the signed setup create route must apply"
    );
    assert_eq!(
        reply.value()?,
        json!({"result":"applied","workspace":{
            "workspaceId":OPERATION,"name":"Research","description":"","state":"active"
        }})
    );
    let rows = fixture.rows(A,
        "SELECT (SELECT COUNT(*) FROM workspaces),(SELECT COUNT(*) FROM request_log),(SELECT COUNT(*) FROM organization_change_records WHERE kind='workspaceCreated')", vec![]).await?;
    assert_eq!(rows, vec![vec![number(1)?, number(1)?, number(1)?]]);
    let member = signed::Person::member(A, signed::NOW_SECONDS)?;
    let lookup = fixture
        .person(
            signed::LOOKUP,
            &json!({"organizationId":A,"workspaceId":OPERATION}),
            &member,
            &signed::lookup_intent(A, OPERATION),
            &signed::fresh_request_id(signed::NOW_SECONDS)?,
        )
        .await?;
    assert_eq!(lookup.status, 200);
    assert_eq!(
        lookup.value()?,
        json!({"result":"found","workspace":{
            "workspaceId":OPERATION,"name":"Research","description":"","state":"active"
        }})
    );
    let before = support::durable_effects(&fixture).await?;
    let retried = support::create(&fixture, &owner, OPERATION, "Research", "").await?;
    assert_eq!(retried.status, 200);
    assert_eq!(
        retried.body, reply.body,
        "a fresh proof replays the exact first answer bytes"
    );
    assert!(
        support::durable_effects(&fixture).await? == before,
        "replay rewrote directory, ledger, audit or actor"
    );
    let reused = fixture
        .person(signed::CREATE, &body, &owner, &intent, &first_proof)
        .await?;
    assert_eq!(reused.status, 200);
    assert_eq!(
        reused.value()?,
        json!({"result":"refused","reason":"requestReused"})
    );
    let changed = support::create(&fixture, &owner, OPERATION, "Different name", "").await?;
    assert_eq!(changed.status, 200);
    assert_eq!(
        changed.value()?,
        json!({"result":"refused","reason":"conflict"})
    );
    assert!(support::durable_effects(&fixture).await? == before);
    let same_name = support::create(
        &fixture,
        &owner,
        "019f0000-0000-7000-8000-00000000d702",
        "Research",
        "",
    )
    .await?;
    assert_eq!(same_name.status, 200);
    assert_eq!(
        same_name.value()?,
        json!({"result":"refused","reason":"nameInUse"})
    );
    assert_eq!(fixture.rows(A,"SELECT (SELECT COUNT(*) FROM workspaces),(SELECT COUNT(*) FROM request_log),(SELECT COUNT(*) FROM organization_change_records WHERE kind='workspaceCreated'),(SELECT COUNT(*) FROM organization_used_requests)",vec![]).await?,vec![vec![number(1)?,number(2)?,number(1)?,number(5)?]]);
    let records = fixture
        .person(
            signed::RECORDS,
            &json!({"organizationId":A,"after":null,"limit":100}),
            &owner,
            &signed::records_intent(A, None, 100),
            &signed::fresh_request_id(signed::NOW_SECONDS)?,
        )
        .await?;
    let records = records.value()?;
    let created = records
        .get("records")
        .and_then(Value::as_array)
        .ok_or("audit list missing")?
        .get(1)
        .ok_or("create record missing")?;
    assert_eq!(created.get("requestId"), Some(&json!(OPERATION)));
    assert_eq!(created.get("targetId"), Some(&json!(OPERATION)));
    assert_eq!(
        created.get("origin"),
        Some(
            &json!({"kind":"person","personId":signed::OWNER,"permissionChecked":"workspace:create"})
        )
    );
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Facts-only expiry and unchanged authorization are observable test assertions, with opaque stamp comparisons."
)]
#[tokio::test]
async fn expired_digest_returns_verified_facts_but_create_stays_refused() -> Result {
    // Arrange: a genuinely signed stored stamp, then move the one injected clock.
    let fixture = Fixture::new().await?;
    fixture.ready().await?;
    let statement = signed::stamp(A, signed::NOW_SECONDS)?;
    let expires = signed::NOW_SECONDS - 60 + signed::LIFETIME_SECONDS;
    let later = expires + 1;
    fixture
        .clock
        .advance_to(runtime_primitives::SyncTime::from_unix_millis(later * 1000))?;
    let before = fixture.snapshot().await?;
    let before_admission = admission_rows(&fixture).await?;

    // Act: this read returns verified facts; it does not grant live standing.
    let digest = fixture.digest(A).await?;
    assert_eq!(digest.status, 200, "expired stored facts remain readable");
    assert_eq!(
        digest.value()?,
        json!({
            "digest":signed::digest(&statement),"expiresAt":expires,"signingKeyId":"stamp-a"
        })
    );
    assert!(expires < fixture.clock_time());
    let owner = signed::Person::owner(A, later)?;
    let body = signed::create_body(A, OPERATION, "Expired authority");
    let intent = signed::create_intent(A, OPERATION, "Expired authority");
    let refused = fixture
        .person(
            signed::CREATE,
            &body,
            &owner,
            &intent,
            &signed::fresh_request_id(later)?,
        )
        .await?;
    assert_eq!(
        refused.status, 503,
        "facts read must not authorize a create"
    );
    assert!(refused.body.is_empty());
    let read = fixture
        .person(
            signed::RECORDS,
            &json!({"organizationId":A,"after":Value::Null,"limit":100}),
            &owner,
            &signed::records_intent(A, None, 100),
            &signed::fresh_request_id(later)?,
        )
        .await?;
    assert_eq!(
        read.status, 503,
        "existing admin admission still needs Valid standing"
    );
    assert!(read.body.is_empty());
    assert!(
        fixture.snapshot().await? == before,
        "facts and refusals changed stored rows"
    );
    assert!(
        admission_rows(&fixture).await? == before_admission,
        "facts and refusals changed directory, ledger, proof or actor rows"
    );
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Facts and refusal assertions use real signed HTTP while fixture errors propagate."
)]
#[tokio::test]
async fn not_yet_valid_digest_returns_facts_without_live_authority() -> Result {
    let fixture = Fixture::new().await?;
    fixture.ready().await?;
    let statement = signed::stamp(A, signed::NOW_SECONDS)?;
    let before_issuance = signed::NOW_SECONDS - 120;
    fixture
        .clock
        .set_utc(chrono::DateTime::from_timestamp(before_issuance, 0).ok_or("fixture UTC range")?);
    let before = admission_rows(&fixture).await?;
    let identity = fixture.snapshot().await?;
    let reply = fixture.digest(A).await?;
    assert_eq!(
        reply.status, 200,
        "stored facts remain readable before issuance too"
    );
    assert_eq!(
        reply.value()?,
        json!({"digest":signed::digest(&statement),
        "expiresAt":signed::NOW_SECONDS-60+signed::LIFETIME_SECONDS,"signingKeyId":"stamp-a"})
    );
    let owner = signed::Person::owner(A, before_issuance)?;
    let body = signed::create_body(A, OPERATION, "Before issuance");
    let intent = signed::create_intent(A, OPERATION, "Before issuance");
    let reply = fixture
        .person(
            signed::CREATE,
            &body,
            &owner,
            &intent,
            &signed::fresh_request_id(before_issuance)?,
        )
        .await?;
    assert_eq!(
        reply.status, 503,
        "facts must not authorize a future organization stamp"
    );
    assert!(reply.body.is_empty());
    assert!(admission_rows(&fixture).await? == before);
    assert!(fixture.snapshot().await? == identity);
    fixture.close().await
}
