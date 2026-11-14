//! Exact B2 cold-trust and first-person-floor carries over the real private Worker.
use super::support::{DIGEST, Fixture, ORG, PROVISION, REVISIONS, org_revision, person_revision};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, ReadOutcome, RowExpectation, SqlValue, Statement,
    StatementBatch, StoreFailure,
};
use collaboration_central_conformance::signin_fixtures::{self as signed, NOW_SECONDS};
use fleet_identity_protocol::OrganizationId;
use serde_json::{Value, json};
use std::time::Duration;

const B: &str = "019f0000-0000-7000-8000-00000000b2b1";
const C: &str = "019f0000-0000-7000-8000-00000000b2c1";
const MOVE_SECONDS: i64 = 3_600;

async fn initialize_clock(f: &Fixture, organization: &str) {
    let organization: OrganizationId = organization.to_owned().try_into().unwrap();
    f.host.central(&organization).await;
}
async fn cold_reset(f: &Fixture, organization: &str) {
    let organization: OrganizationId = organization.to_owned().try_into().unwrap();
    let runtime = f.host.central(&organization).await;
    let reset: Result<bool, StoreFailure> = runtime.control("crash", &()).await;
    assert!(reset.is_err(), "cold reset must destroy its own request");
    f.host.central(&organization).await;
}
async fn raw_read(f: &Fixture, sql: &'static str) -> ReadOutcome {
    let result: Result<ReadOutcome, StoreFailure> = f
        .runtime()
        .await
        .control("raw-read", &Statement { sql, args: vec![] })
        .await
        .unwrap();
    result.unwrap()
}
async fn snapshot(f: &Fixture) -> (ReadOutcome, ReadOutcome) {
    (raw_read(f,"SELECT id,organization_statement,created_at,updated_at FROM organization_identity ORDER BY id").await,
     raw_read(f,"SELECT * FROM organization_change_records ORDER BY id").await)
}
async fn schema(f: &Fixture, sql: &'static str) {
    let outcome = f
        .runtime()
        .await
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement { sql, args: vec![] },
                expect: RowExpectation::Any,
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(outcome, BatchOutcome::Applied { .. }));
}

#[tokio::test]
async fn cold_tampered_stamp_refuses_valid_setup_and_keeps_an_unaffected_org_readable() {
    let f = Fixture::new().await;
    initialize_clock(&f, B).await;
    let original = signed::stamp(ORG, NOW_SECONDS).unwrap();
    assert_eq!(f.provision().await.0, 200);
    assert_eq!(f.post(PROVISION,&json!({"organizationId":B,"organizationStatement":signed::stamp(B,NOW_SECONDS).unwrap()})).await.0,200);
    f.execute(
        "UPDATE organization_identity SET organization_statement=?",
        vec![SqlValue::Text(signed::tamper_signature(&original).unwrap())],
    )
    .await;
    let before = snapshot(&f).await;
    cold_reset(&f, ORG).await;
    cold_reset(&f, B).await;
    assert_eq!(
        f.post(DIGEST, &json!({"organizationId":ORG})).await,
        (503, Value::Null)
    );
    assert_eq!(
        f.provision().await.0,
        409,
        "a setup retry must not answer over a refused stamp"
    );
    assert_eq!(
        f.post(DIGEST, &json!({"organizationId":B})).await.0,
        200,
        "the untouched organization's stamp remains usable after cold reopen"
    );
    assert!(
        snapshot(&f).await == before,
        "refusing the edited stamp must rewrite nothing"
    );
    f.close().await;
}

#[tokio::test]
async fn injected_clock_reaches_a_never_named_org_and_a_live_org_renewal_then_expiry() {
    let f = Fixture::new().await;
    let first = signed::stamp(ORG, NOW_SECONDS).unwrap();
    assert_eq!(f.provision().await.0, 200);
    let start = NOW_SECONDS * 1000;
    assert_eq!(
        f.read("SELECT created_at,updated_at FROM organization_change_records ORDER BY id")
            .await
            .rows,
        vec![vec![
            SqlValue::Integer(start.try_into().unwrap()),
            SqlValue::Integer(start.try_into().unwrap())
        ]]
    );
    // No B control or private request has happened before this clock advance.
    f.clock
        .advance(Duration::from_secs(MOVE_SECONDS.try_into().unwrap()))
        .unwrap();
    let moved = (NOW_SECONDS + MOVE_SECONDS) * 1000;
    initialize_clock(&f, B).await;
    let later = signed::first_statement(B, NOW_SECONDS, MOVE_SECONDS).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":B,"organizationStatement":later})
        )
        .await
        .0,
        200,
        "B first opens after the move and uses that moved clock"
    );
    let b: OrganizationId = B.to_owned().try_into().unwrap();
    let b_runtime = f.host.central(&b).await;
    assert_eq!(
        b_runtime
            .read(Statement {
                sql: "SELECT created_at,updated_at FROM organization_change_records ORDER BY id",
                args: vec![]
            })
            .await
            .unwrap()
            .rows,
        vec![vec![
            SqlValue::Integer(moved.try_into().unwrap()),
            SqlValue::Integer(moved.try_into().unwrap())
        ]]
    );
    f.runtime().await; // Move the already-live A to the same injected sample.
    let issued = NOW_SECONDS + MOVE_SECONDS - 10;
    let expiry = issued + 86_400;
    let mut claims = signed::claims(ORG, NOW_SECONDS);
    signed::set_claim(&mut claims, "iat", json!(issued)).unwrap();
    signed::set_claim(&mut claims, "exp", json!(expiry)).unwrap();
    signed::set_claim(&mut claims, "replaces", json!(signed::digest(&first))).unwrap();
    let renewal = signed::sign(&signed::stamp_header(), &claims, 7).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":renewal})
        )
        .await
        .0,
        200,
        "the live org accepts a renewal issued after t0"
    );
    assert_eq!(
        f.read("SELECT created_at,updated_at FROM organization_change_records ORDER BY id")
            .await
            .rows,
        vec![
            vec![
                SqlValue::Integer(start.try_into().unwrap()),
                SqlValue::Integer(start.try_into().unwrap())
            ],
            vec![
                SqlValue::Integer(moved.try_into().unwrap()),
                SqlValue::Integer(moved.try_into().unwrap())
            ]
        ],
        "A has exactly its start record and moved-time renewal record"
    );
    assert_eq!(
        f.read("SELECT created_at,updated_at FROM organization_identity")
            .await
            .rows,
        vec![vec![
            SqlValue::Integer(start.try_into().unwrap()),
            SqlValue::Integer(moved.try_into().unwrap())
        ]]
    );
    assert_eq!(
        b_runtime
            .read(Statement {
                sql: "SELECT created_at,updated_at FROM organization_identity",
                args: vec![]
            })
            .await
            .unwrap()
            .rows,
        vec![vec![
            SqlValue::Integer(moved.try_into().unwrap()),
            SqlValue::Integer(moved.try_into().unwrap())
        ]]
    );
    assert_eq!(f.post(DIGEST, &json!({"organizationId":ORG})).await.0, 200);
    f.clock
        .advance(Duration::from_secs(
            (expiry - (NOW_SECONDS + MOVE_SECONDS)).try_into().unwrap(),
        ))
        .unwrap();
    f.runtime().await;
    initialize_clock(&f, B).await;
    assert_eq!(
        f.post(DIGEST, &json!({"organizationId":ORG})).await,
        (
            200,
            json!({"digest":signed::digest(&renewal),"expiresAt":expiry,"signingKeyId":"stamp-a"})
        ),
        "the live org returns verified stored facts exactly at exp without granting authority"
    );
    assert_eq!(
        f.post(DIGEST, &json!({"organizationId":B})).await.0,
        200,
        "B's longer-lived stamp remains valid at A's expiry"
    );
    f.close().await;
}

#[tokio::test]
async fn cold_expired_valid_stamp_refuses_and_an_unaffected_organization_stays_readable() {
    let f = Fixture::new().await;
    initialize_clock(&f, B).await;
    let mut claims = signed::claims(ORG, NOW_SECONDS);
    signed::set_claim(&mut claims, "exp", json!(NOW_SECONDS + 2)).unwrap();
    let short = signed::sign(&signed::stamp_header(), &claims, 7).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":short})
        )
        .await
        .0,
        200
    );
    assert_eq!(f.post(PROVISION,&json!({"organizationId":B,"organizationStatement":signed::stamp(B,NOW_SECONDS).unwrap()})).await.0,200);
    assert_eq!(f.post(DIGEST, &json!({"organizationId":ORG})).await.0, 200);
    assert_eq!(f.post(DIGEST, &json!({"organizationId":B})).await.0, 200);
    let before = snapshot(&f).await;
    f.clock.advance(Duration::from_secs(2)).unwrap();
    cold_reset(&f, ORG).await;
    cold_reset(&f, B).await;
    assert_eq!(
        f.post(DIGEST, &json!({"organizationId":ORG})).await,
        (
            200,
            json!({"digest":signed::digest(&short),"expiresAt":NOW_SECONDS+2,"signingKeyId":"stamp-a"})
        ),
        "a valid stored stamp observed cold at exp still returns facts"
    );
    assert_eq!(
        f.post(DIGEST, &json!({"organizationId":B})).await.0,
        200,
        "the unaffected organization's stamp remains readable"
    );
    assert!(
        snapshot(&f).await == before,
        "cold expiry rewrites neither stamp nor records"
    );
    f.close().await;
}

#[tokio::test]
async fn cold_retired_key_refuses_exact_retry_and_new_org_but_preserves_the_remaining_org() {
    let f = Fixture::new().await;
    initialize_clock(&f, B).await;
    initialize_clock(&f, C).await;
    let retiring = signed::stamp_under_second_key(ORG, NOW_SECONDS).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":retiring})
        )
        .await
        .0,
        200
    );
    assert_eq!(f.post(PROVISION,&json!({"organizationId":B,"organizationStatement":signed::stamp(B,NOW_SECONDS).unwrap()})).await.0,200);
    assert_eq!(f.post(DIGEST, &json!({"organizationId":ORG})).await.0, 200);
    let before = snapshot(&f).await;
    f.keys.retire("stamp-b").unwrap();
    cold_reset(&f, ORG).await;
    cold_reset(&f, B).await;
    assert_eq!(
        f.post(DIGEST, &json!({"organizationId":ORG})).await,
        (503, Value::Null)
    );
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":retiring})
        )
        .await
        .0,
        422,
        "the exact setup retry no longer verifies under the retired key"
    );
    assert_eq!(
        f.provision().await.0,
        409,
        "a remaining-key setup cannot revive the refused stored stamp"
    );
    assert_eq!(
        f.post(REVISIONS, &org_revision(1, "active")).await,
        (503, Value::Null)
    );
    initialize_clock(&f, C).await;
    assert_eq!(f.post(PROVISION,&json!({"organizationId":C,"organizationStatement":signed::stamp_under_second_key(C,NOW_SECONDS).unwrap()})).await.0,422,"a new organization under the retired key is refused");
    assert_eq!(
        f.post(DIGEST, &json!({"organizationId":B})).await.0,
        200,
        "the remaining key's stored stamp still works"
    );
    let after = snapshot(&f).await;
    assert!(
        after == before,
        "A keeps exactly its first stamp and record"
    );
    assert_eq!((after.0.rows.len(), after.1.rows.len()), (1, 1));
    let c: OrganizationId = C.to_owned().try_into().unwrap();
    let c_runtime = f.host.central(&c).await;
    let c_rows=c_runtime.read(Statement{sql:"SELECT (SELECT COUNT(*) FROM organization_identity),(SELECT COUNT(*) FROM organization_change_records)",args:vec![]}).await.unwrap();
    assert_eq!(
        c_rows.rows,
        vec![vec![
            SqlValue::Integer(0.try_into().unwrap()),
            SqlValue::Integer(0.try_into().unwrap())
        ]],
        "nothing is written for C"
    );
    f.close().await;
}

#[tokio::test]
async fn cold_foreign_compact_with_the_correct_row_id_refuses_every_stamped_call() {
    let f = Fixture::new().await;
    assert_eq!(f.provision().await.0, 200);
    for statement in [
        Statement {
            sql: "SELECT 1",
            args: vec![],
        },
        Statement {
            sql: "SELECT id,organization_statement,created_at,updated_at FROM organization_identity ORDER BY id",
            args: vec![SqlValue::Text(ORG.into())],
        },
    ] {
        let refused: Result<Result<ReadOutcome, StoreFailure>, StoreFailure> =
            f.runtime().await.control("raw-read", &statement).await;
        assert!(
            refused.is_err(),
            "raw corruption snapshots must refuse arbitrary SQL or arguments"
        );
    }
    let foreign = signed::stamp(B, NOW_SECONDS).unwrap();
    // The routing-bug shape keeps row.id=A and changes ONLY the compact to B.
    f.execute(
        "UPDATE organization_identity SET organization_statement=?",
        vec![SqlValue::Text(foreign.clone())],
    )
    .await;
    let before = snapshot(&f).await;
    let counts_before = f.counts().await;
    let workspaces_before = f
        .read("SELECT * FROM workspaces ORDER BY workspace_id")
        .await;
    let requests_before = f
        .read("SELECT * FROM request_log ORDER BY request_id")
        .await;
    assert!(
        before.0.rows.first().and_then(|row| row.first()) == Some(&SqlValue::Text(ORG.into())),
        "the stored row remains addressed to A"
    );
    cold_reset(&f, ORG).await;
    assert_eq!(
        f.post(DIGEST, &json!({"organizationId":ORG})).await,
        (503, Value::Null)
    );
    assert_eq!(
        f.provision().await.0,
        409,
        "valid A setup is refused over the untrusted B compact"
    );
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":foreign})
        )
        .await
        .0,
        403,
        "foreign B statement offered to A is refused"
    );
    assert_eq!(
        f.post(REVISIONS, &org_revision(1, "active")).await,
        (503, Value::Null)
    );
    assert_eq!(f.records(&f.owner()).await, (503, Value::Null));
    assert!(
        snapshot(&f).await == before,
        "refusing each stamped call must rewrite neither identity nor records"
    );
    assert_eq!(
        f.counts().await,
        counts_before,
        "refused stamped calls must add no floor or person-proof marker"
    );
    assert!(
        f.read("SELECT * FROM workspaces ORDER BY workspace_id")
            .await
            == workspaces_before,
        "refused private calls must leave every owning workspace row unchanged"
    );
    assert!(
        f.read("SELECT * FROM request_log ORDER BY request_id")
            .await
            == requests_before,
        "refused private calls must leave the owning request log unchanged"
    );
    f.close().await;
}

#[tokio::test]
async fn first_person_floor_insert_failure_never_acknowledges_or_changes_the_org_floor() {
    let f = Fixture::new().await;
    assert_eq!(f.provision().await.0, 200);
    assert_eq!(
        f.post(REVISIONS, &org_revision(3, "active")).await,
        (200, json!({"acknowledgedRevision":3}))
    );
    let before = f
        .read("SELECT * FROM organization_authorization_floors ORDER BY id")
        .await;
    assert!(
        f.read("SELECT id FROM person_organization_authorization_floors")
            .await
            .rows
            .is_empty()
    );
    schema(&f,"CREATE TRIGGER fail_first_person_floor BEFORE INSERT ON person_organization_authorization_floors BEGIN SELECT RAISE(ABORT,'fixture first person floor failure'); END").await;
    let request = person_revision(4, "active");
    let failed = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("{}{REVISIONS}", f.host.signin_origin()))
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(
        failed
            .headers()
            .get("x-collaboration-central-entry")
            .unwrap(),
        "collaboration-central-link-entry"
    );
    assert_eq!(
        failed.status().as_u16(),
        503,
        "a rolled-back apply never acknowledges"
    );
    assert!(failed.bytes().await.unwrap().is_empty());
    assert!(
        f.read("SELECT id FROM person_organization_authorization_floors")
            .await
            .rows
            .is_empty(),
        "the faulted apply left no person floor behind"
    );
    assert_eq!(
        f.read("SELECT * FROM organization_authorization_floors ORDER BY id")
            .await,
        before,
        "the acknowledged organization floor remains unchanged"
    );
    schema(&f, "DROP TRIGGER fail_first_person_floor").await;
    assert_eq!(
        f.post(REVISIONS, &request).await,
        (200, json!({"acknowledgedRevision":4}))
    );
    assert_eq!(f.read("SELECT o.minimum_revision,o.state,p.minimum_revision,p.id FROM organization_authorization_floors o,person_organization_authorization_floors p").await.rows,vec![vec![SqlValue::Integer(3.try_into().unwrap()),SqlValue::Text("active".into()),SqlValue::Integer(4.try_into().unwrap()),SqlValue::Text(signed::OWNER_SUBJECT.into())]]);
    let person_before = f
        .read("SELECT * FROM person_organization_authorization_floors ORDER BY id")
        .await;
    let mut changed_subject = person_revision(5, "active");
    signed::set_claim(
        &mut changed_subject,
        "subjectId",
        json!(signed::MEMBER_SUBJECT),
    )
    .unwrap();
    assert_eq!(
        f.post(REVISIONS, &changed_subject).await,
        (409, Value::Null),
        "a changed subject for the same org/person is refused"
    );
    assert_eq!(
        f.read("SELECT * FROM person_organization_authorization_floors ORDER BY id")
            .await,
        person_before
    );
    assert_eq!(
        f.read("SELECT * FROM organization_authorization_floors ORDER BY id")
            .await,
        before
    );
    f.close().await;
}
