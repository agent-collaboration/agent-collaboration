//! Missing literal B2 cold-trust and first-person-floor assertions.
use super::support::*;
use serde_json::json;
use std::time::Duration;

const C: &str = "019f0000-0000-7000-8000-00000000b2c1";
const MOVE_SECONDS: i64 = 3_600;

#[expect(
    clippy::panic_in_result_fn,
    reason = "The original cold-tamper carry retains both valid-setup refusal and the unaffected organization's digest control."
)]
#[tokio::test]
async fn cold_tampered_stamp_refuses_valid_setup_and_keeps_an_unaffected_org_readable() -> Result {
    let mut f = Fixture::new().await?;
    let original = signed::stamp(A, signed::NOW_SECONDS)?;
    assert_eq!(f.provision(A, &original).await?.status, 200);
    assert_eq!(
        f.provision(B, &signed::stamp(B, signed::NOW_SECONDS)?)
            .await?
            .status,
        200
    );
    f.apply(
        A,
        "UPDATE organization_identity SET organization_statement=?",
        vec![text(signed::tamper_signature(&original)?)],
    )
    .await?;
    let before = f.snapshot().await?;
    f.reopen().await?;
    assert_eq!(f.digest(A).await?.status, 503);
    assert_eq!(
        f.provision(A, &original).await?.status,
        409,
        "a setup retry must not answer over a refused stamp"
    );
    assert_eq!(
        f.digest(B).await?.status,
        200,
        "the untouched organization's stamp remains usable after cold reopen"
    );
    assert!(
        f.snapshot().await? == before,
        "refusing the edited stamp must rewrite nothing"
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The original injected-clock carry reaches existing and newly opened orgs and controls their actual record times."
)]
#[tokio::test]
async fn injected_clock_reaches_a_never_named_org_and_a_live_org_renewal_then_expiry() -> Result {
    let f = Fixture::new().await?;
    let first = signed::stamp(A, signed::NOW_SECONDS)?;
    assert_eq!(f.provision(A, &first).await?.status, 200);
    let start = signed::NOW_SECONDS * 1000;
    assert_eq!(
        f.rows(
            A,
            "SELECT created_at,updated_at FROM organization_change_records ORDER BY id",
            vec![]
        )
        .await?,
        vec![vec![number(start)?, number(start)?]]
    );
    let b_file = f.root.path().join("central").join(format!("{B}.sqlite"));
    assert!(
        !b_file.exists(),
        "B has never been named before the clock moves"
    );
    f.clock
        .advance(Duration::from_secs(MOVE_SECONDS.try_into()?))?;
    let moved = (signed::NOW_SECONDS + MOVE_SECONDS) * 1000;
    assert!(!b_file.exists(), "advancing the clock must not open B");
    let later = signed::first_statement(B, signed::NOW_SECONDS, MOVE_SECONDS)?;
    assert_eq!(
        f.provision(B, &later).await?.status,
        200,
        "an org first opened after the move uses the moved clock"
    );
    assert_eq!(
        f.rows(
            B,
            "SELECT created_at,updated_at FROM organization_change_records ORDER BY id",
            vec![]
        )
        .await?,
        vec![vec![number(moved)?, number(moved)?]]
    );
    let issued = signed::NOW_SECONDS + MOVE_SECONDS - 10;
    let expiry = issued + 86_400;
    let mut claims = signed::claims(A, signed::NOW_SECONDS);
    signed::set_claim(&mut claims, "iat", json!(issued))?;
    signed::set_claim(&mut claims, "exp", json!(expiry))?;
    signed::set_claim(&mut claims, "replaces", json!(signed::digest(&first)))?;
    let renewal = signed::sign(&signed::stamp_header(), &claims, 7)?;
    assert_eq!(
        f.provision(A, &renewal).await?.status,
        200,
        "the live org accepts a renewal issued after t0"
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT created_at,updated_at FROM organization_change_records ORDER BY id",
            vec![]
        )
        .await?,
        vec![
            vec![number(start)?, number(start)?],
            vec![number(moved)?, number(moved)?]
        ],
        "A has exactly its start record and moved-time renewal record"
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT created_at,updated_at FROM organization_identity",
            vec![]
        )
        .await?,
        vec![vec![number(start)?, number(moved)?]]
    );
    assert_eq!(
        f.rows(
            B,
            "SELECT created_at,updated_at FROM organization_identity",
            vec![]
        )
        .await?,
        vec![vec![number(moved)?, number(moved)?]]
    );
    assert_eq!(f.digest(A).await?.status, 200);
    f.clock.advance(Duration::from_secs(
        (expiry - (signed::NOW_SECONDS + MOVE_SECONDS)).try_into()?,
    ))?;
    let facts = f.digest(A).await?;
    assert_eq!(
        facts.status, 200,
        "R29 facts remain readable exactly at exp"
    );
    assert_eq!(
        facts.value()?,
        json!({"digest":signed::digest(&renewal),"expiresAt":expiry,"signingKeyId":"stamp-a"})
    );
    let denied = f
        .post(REVISIONS, &organization_floor(A, 1, "active"))
        .await?;
    assert_eq!(
        denied.status, 503,
        "facts read must not authorize a revision push at expiry"
    );
    assert!(denied.body.is_empty());
    assert_eq!(
        f.digest(B).await?.status,
        200,
        "B's longer-lived stamp remains valid at A's expiry"
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "R29 returns verified stored facts at expiry after cold reopen, while authorization remains refused."
)]
#[tokio::test]
async fn cold_expired_valid_stamp_refuses_and_an_unaffected_organization_stays_readable() -> Result
{
    let mut f = Fixture::new().await?;
    let mut claims = signed::claims(A, signed::NOW_SECONDS);
    signed::set_claim(&mut claims, "exp", json!(signed::NOW_SECONDS + 2))?;
    let short = signed::sign(&signed::stamp_header(), &claims, 7)?;
    assert_eq!(f.provision(A, &short).await?.status, 200);
    assert_eq!(
        f.provision(B, &signed::stamp(B, signed::NOW_SECONDS)?)
            .await?
            .status,
        200
    );
    assert_eq!(
        (f.digest(A).await?.status, f.digest(B).await?.status),
        (200, 200)
    );
    let before = f.snapshot().await?;
    f.clock.advance(Duration::from_secs(2))?;
    f.reopen().await?;
    let facts = f.digest(A).await?;
    assert_eq!(
        facts.status, 200,
        "a valid stored stamp's facts remain readable cold at exp"
    );
    assert_eq!(
        facts.value()?,
        json!({"digest":signed::digest(&short),"expiresAt":signed::NOW_SECONDS+2,"signingKeyId":"stamp-a"})
    );
    let denied = f
        .post(REVISIONS, &organization_floor(A, 1, "active"))
        .await?;
    assert_eq!(
        denied.status, 503,
        "cold facts read grants no Valid standing"
    );
    assert!(denied.body.is_empty());
    assert_eq!(
        f.digest(B).await?.status,
        200,
        "expiry in A leaves B's still-valid stamp usable"
    );
    assert!(
        f.snapshot().await? == before,
        "cold expiry must not rewrite A's identity or records"
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "All original cold-retired-key refusal and remaining-key controls use the real private link."
)]
#[tokio::test]
async fn cold_retired_key_refuses_exact_retry_and_new_org_but_preserves_the_remaining_org() -> Result
{
    let mut f = Fixture::new().await?;
    let retiring = signed::stamp_under_second_key(A, signed::NOW_SECONDS)?;
    assert_eq!(f.provision(A, &retiring).await?.status, 200);
    assert_eq!(
        f.provision(B, &signed::stamp(B, signed::NOW_SECONDS)?)
            .await?
            .status,
        200
    );
    assert_eq!(f.digest(A).await?.status, 200);
    let before = f.snapshot().await?;
    f.keys.retire("stamp-b")?;
    f.reopen().await?;
    assert_eq!(
        f.digest(A).await?.status,
        503,
        "a stored stamp under the retired key is refused"
    );
    assert_eq!(
        f.provision(A, &retiring).await?.status,
        422,
        "the exact setup retry no longer verifies under its retired key"
    );
    assert_eq!(
        f.provision(A, &signed::stamp(A, signed::NOW_SECONDS)?)
            .await?
            .status,
        409,
        "a remaining-key setup cannot revive the refused stamp"
    );
    assert_eq!(
        f.post(REVISIONS, &organization_floor(A, 1, "active"))
            .await?
            .status,
        503,
        "no floor reaches a refused stamp"
    );
    assert_eq!(
        f.provision(C, &signed::stamp_under_second_key(C, signed::NOW_SECONDS)?)
            .await?
            .status,
        422,
        "a new organization under the retired key is refused"
    );
    assert_eq!(
        f.digest(B).await?.status,
        200,
        "the remaining key's already-stored stamp still works"
    );
    let after = f.snapshot().await?;
    assert!(after == before, "the refused stamp stays exactly as stored");
    assert_eq!((after.0.len(), after.1.len()), (1, 1));
    assert_eq!(f.rows(C,"SELECT (SELECT COUNT(*) FROM organization_identity),(SELECT COUNT(*) FROM organization_change_records)",vec![]).await?,vec![vec![number(0)?,number(0)?]],"nothing is written for C");
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The original first-person-floor failure must leave no row behind before any retry can hide a partial write."
)]
#[tokio::test]
async fn first_person_floor_insert_failure_never_acknowledges_or_changes_the_org_floor() -> Result {
    let f = Fixture::new().await?;
    assert_eq!(
        f.provision(A, &signed::stamp(A, signed::NOW_SECONDS)?)
            .await?
            .status,
        200
    );
    let acknowledged = f
        .post(REVISIONS, &organization_floor(A, 3, "active"))
        .await?;
    assert_eq!(acknowledged.status, 200);
    assert_eq!(acknowledged.value()?, json!({"acknowledgedRevision":3}));
    let organization_before = f
        .rows(
            A,
            "SELECT * FROM organization_authorization_floors ORDER BY id",
            vec![],
        )
        .await?;
    assert!(
        f.rows(
            A,
            "SELECT id FROM person_organization_authorization_floors",
            vec![]
        )
        .await?
        .is_empty()
    );
    f.apply(A,"CREATE TRIGGER fail_first_person_floor BEFORE INSERT ON person_organization_authorization_floors BEGIN SELECT RAISE(ABORT,'fixture first person floor failure'); END",vec![]).await?;
    let request = person_floor(A, signed::OWNER, signed::OWNER_SUBJECT, 4, "active");
    let failed = f.post(REVISIONS, &request).await?;
    assert_eq!(failed.status, 503, "a rolled-back apply never acknowledges");
    assert!(failed.body.is_empty());
    assert!(
        f.rows(
            A,
            "SELECT id FROM person_organization_authorization_floors",
            vec![]
        )
        .await?
        .is_empty(),
        "the faulted apply left no person floor behind"
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT * FROM organization_authorization_floors ORDER BY id",
            vec![]
        )
        .await?,
        organization_before,
        "the acknowledged organization floor remains unchanged"
    );
    f.apply(A, "DROP TRIGGER fail_first_person_floor", vec![])
        .await?;
    let retried = f.post(REVISIONS, &request).await?;
    assert_eq!(retried.status, 200);
    assert_eq!(retried.value()?, json!({"acknowledgedRevision":4}));
    assert_eq!(f.rows(A,"SELECT o.minimum_revision,o.state,p.minimum_revision,p.id FROM organization_authorization_floors o,person_organization_authorization_floors p",vec![]).await?,vec![vec![number(3)?,text("active"),number(4)?,text(signed::OWNER_SUBJECT)]]);
    let person_before = f
        .rows(
            A,
            "SELECT * FROM person_organization_authorization_floors ORDER BY id",
            vec![],
        )
        .await?;
    let changed_subject = f
        .post(
            REVISIONS,
            &person_floor(A, signed::OWNER, signed::MEMBER_SUBJECT, 5, "active"),
        )
        .await?;
    assert_eq!(
        changed_subject.status, 409,
        "a changed subject for the same org/person cannot replace the retained floor"
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT * FROM person_organization_authorization_floors ORDER BY id",
            vec![]
        )
        .await?,
        person_before
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT * FROM organization_authorization_floors ORDER BY id",
            vec![]
        )
        .await?,
        organization_before
    );
    f.close().await
}
