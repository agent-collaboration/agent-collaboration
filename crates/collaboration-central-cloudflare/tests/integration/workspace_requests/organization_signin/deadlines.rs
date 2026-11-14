//! Real Worker read admission at the signed applying-time boundaries.
use super::support::{Fixture, ORG, RECORDS, REVISIONS, org_revision, person_revision};
use central_storage::{ReadOutcome, SqlValue};
use collaboration_central_conformance::signin_fixtures::{self as signed, NOW_SECONDS};
use serde_json::{Value, json};

const BOUND_MS: i64 = (NOW_SECONDS + 30) * 1000;
fn set(claims: &mut Value, name: &str, value: Value) {
    claims
        .as_object_mut()
        .unwrap()
        .insert(name.to_owned(), value);
}
fn token_bound(claims: &mut Value) {
    set(claims, "exp", json!(NOW_SECONDS + 30));
    set(
        claims,
        "session_expires_at_ms",
        json!((NOW_SECONDS + 60) * 1000),
    );
    set(
        claims,
        "authorization_snapshot_valid_until_ms",
        json!((NOW_SECONDS + 90) * 1000),
    );
}
fn session_bound(claims: &mut Value) {
    set(claims, "exp", json!(NOW_SECONDS + 90));
    set(claims, "session_expires_at_ms", json!(BOUND_MS));
    set(
        claims,
        "authorization_snapshot_valid_until_ms",
        json!((NOW_SECONDS + 60) * 1000),
    );
}
fn snapshot_bound(claims: &mut Value) {
    set(claims, "exp", json!(NOW_SECONDS + 90));
    set(
        claims,
        "session_expires_at_ms",
        json!((NOW_SECONDS + 60) * 1000),
    );
    set(
        claims,
        "authorization_snapshot_valid_until_ms",
        json!(BOUND_MS),
    );
}
fn number(value: i64) -> SqlValue {
    SqlValue::Integer(value.try_into().unwrap())
}
async fn counts(f: &Fixture) -> ReadOutcome {
    f.read("SELECT (SELECT COUNT(*) FROM workspaces),(SELECT COUNT(*) FROM organization_change_records),(SELECT COUNT(*) FROM organization_used_requests),(SELECT COUNT(*) FROM request_log)").await
}
async fn ready(f: &Fixture) {
    assert_eq!(f.provision().await.0, 200);
    f.floors().await;
}
async fn held_read(f: &Fixture, adjust: fn(&mut Value), release_after_ms: i64) -> (u16, Value) {
    let person = f.owner();
    let body = json!({"organizationId":ORG,"after":null,"limit":100});
    let headers = person
        .headers_with(
            RECORDS,
            &signed::records_intent(ORG, None, 100),
            &signed::fresh_request_id(NOW_SECONDS).unwrap(),
            NOW_SECONDS,
            &adjust,
        )
        .unwrap();
    let runtime = f.runtime().await;
    assert!(
        runtime
            .control::<bool>("hold-central", &"beforeSignInSample")
            .await
            .unwrap()
    );
    let release = async {
        assert!(
            runtime
                .control::<bool>("until-held-central", &"beforeSignInSample")
                .await
                .unwrap()
        );
        f.clock.set_utc(
            chrono::DateTime::from_timestamp_millis(NOW_SECONDS * 1000 + release_after_ms).unwrap(),
        );
        // Transport this one scenario clock to the real Worker before releasing admission.
        f.runtime().await;
        assert!(
            runtime
                .control::<bool>("release-central", &"beforeSignInSample")
                .await
                .unwrap()
        );
    };
    let (answer, ()) = tokio::join!(f.post_with(RECORDS, &body, &headers), release);
    answer
}

#[tokio::test]
async fn each_signed_deadline_refuses_at_the_bound_and_admits_a_read_one_millisecond_before() {
    type ClaimsAdjustment = fn(&mut Value);
    let cases: [(&str, ClaimsAdjustment); 3] = [
        ("token exp", token_bound),
        ("session deadline", session_bound),
        ("snapshot deadline", snapshot_bound),
    ];
    for (bound, adjust) in cases {
        for (released_after, refused) in [(30_000, true), (29_999, false)] {
            let f = Fixture::new().await;
            ready(&f).await;
            let before = counts(&f).await;
            let answer = held_read(&f, adjust, released_after).await;
            let after = counts(&f).await;
            assert_eq!(answer.0, 200);
            if refused {
                assert_eq!(
                    answer.1,
                    json!({"result":"refused","reason":"expired"}),
                    "{bound}: applying sample reached the deadline"
                );
                assert_eq!(
                    after, before,
                    "{bound}: refusal reserved, decided or wrote nothing"
                );
            } else {
                assert_eq!(
                    answer.1.get("result"),
                    Some(&json!("records")),
                    "{bound}: applying sample is one millisecond before the deadline"
                );
                assert_eq!(
                    after.rows,
                    vec![vec![number(0), number(1), number(1), number(0)]],
                    "the admitted read reserves one proof without a held workspace effect"
                );
            }
            f.close().await;
        }
    }
}

#[tokio::test]
async fn applying_request_window_refuses_exactly_sixty_five_seconds_old_and_admits_before() {
    // Both the proof iat and UUIDv7 jti predate NOW by exactly one second.
    for (released_after, refused) in [(64_000, true), (63_999, false)] {
        let f = Fixture::new().await;
        ready(&f).await;
        let before = counts(&f).await;
        let answer = held_read(&f, |_| {}, released_after).await;
        assert_eq!(answer.0, 200);
        if refused {
            assert_eq!(answer.1, json!({"result":"refused","reason":"requestTime"}));
            assert_eq!(
                counts(&f).await,
                before,
                "out-of-window proof reserved nothing"
            );
        } else {
            assert_eq!(answer.1.get("result"), Some(&json!("records")));
        }
        f.close().await;
    }
}

async fn assert_read_refused(f: &Fixture, person: &signed::Person, case: &str, expected: &str) {
    let before = counts(f).await;
    assert_eq!(
        f.records(person).await,
        (200, json!({"result":"refused","reason":expected})),
        "{case}"
    );
    assert_eq!(
        counts(f).await,
        before,
        "{case}: refusal changed no business, proof or decision rows"
    );
}

async fn assert_healthy_read(f: &Fixture) {
    let answer = f.records(&f.owner()).await;
    assert_eq!(answer.0, 200);
    assert_eq!(answer.1.get("result"), Some(&json!("records")));
}

#[tokio::test]
async fn missing_floors_wrong_subject_policy_and_stopped_or_deleted_org_refuse_reads() {
    let f = Fixture::new().await;
    assert_eq!(f.provision().await.0, 200);
    assert_read_refused(
        &f,
        &f.owner(),
        "organization missing",
        "authorizationSyncRequired",
    )
    .await;
    assert_eq!(f.post(REVISIONS, &org_revision(3, "active")).await.0, 200);
    assert_read_refused(
        &f,
        &f.owner(),
        "person missing",
        "authorizationSyncRequired",
    )
    .await;
    assert_eq!(
        f.post(REVISIONS, &person_revision(4, "active")).await.0,
        200
    );

    type AdjustPerson = fn(&mut signed::Person);
    let claim_cases: [(&str, &str, AdjustPerson); 6] = [
        ("wrong subject", "notPermitted", |person| {
            person.subject = signed::MEMBER_SUBJECT.into()
        }),
        ("policy changed", "refreshRequired", |person| {
            person.policy_digest =
                "0000000000000000000000000000000000000000000000000000000000000000".into()
        }),
        ("organization below", "refreshRequired", |person| {
            person.organization_revision = 2
        }),
        (
            "organization ahead",
            "authorizationSyncRequired",
            |person| person.organization_revision = 4,
        ),
        ("person below", "refreshRequired", |person| {
            person.revision = 3
        }),
        ("person ahead", "authorizationSyncRequired", |person| {
            person.revision = 5
        }),
    ];
    for (case, expected, adjust) in claim_cases {
        assert_healthy_read(&f).await;
        let mut person = f.owner();
        adjust(&mut person);
        assert_read_refused(&f, &person, case, expected).await;
    }

    assert_healthy_read(&f).await;
    assert_eq!(
        f.post(REVISIONS, &person_revision(5, "inactive")).await.0,
        200
    );
    assert_read_refused(&f, &f.owner(), "inactive person", "notPermitted").await;
    f.close().await;

    for case in ["stopped", "deleted"] {
        let f = Fixture::new().await;
        ready(&f).await;
        assert_healthy_read(&f).await;
        assert_eq!(f.post(REVISIONS, &org_revision(4, case)).await.0, 200);
        let mut person = f.owner();
        person.organization_revision = 4;
        assert_read_refused(&f, &person, case, "notPermitted").await;
        f.close().await;
    }
}

#[tokio::test]
async fn audience_array_must_name_admin_and_a_refusal_reserves_nothing() {
    let f = Fixture::new().await;
    ready(&f).await;
    let person = f.owner();
    let before = counts(&f).await;
    for (audience, status) in [
        (json!(["https://other.example.invalid/admin"]), 401),
        (
            json!([
                "https://other.example.invalid/admin",
                signed::ADMIN_AUDIENCE
            ]),
            200,
        ),
    ] {
        let headers = person
            .headers_with(
                RECORDS,
                &signed::records_intent(ORG, None, 100),
                &signed::fresh_request_id(NOW_SECONDS).unwrap(),
                NOW_SECONDS,
                &|claims| set(claims, "aud", audience.clone()),
            )
            .unwrap();
        let mut request = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("{}{RECORDS}", f.host.signin_origin()))
            .json(&json!({"organizationId":ORG,"after":null,"limit":100}));
        for (name, value) in &headers {
            request = request.header(*name, value);
        }
        let response = request.send().await.unwrap();
        assert_eq!(
            response
                .headers()
                .get("x-collaboration-central-entry")
                .unwrap(),
            "collaboration-central-link-entry"
        );
        assert_eq!(response.status().as_u16(), status);
        let bytes = response.bytes().await.unwrap();
        if status == 401 {
            assert!(
                bytes.is_empty(),
                "the signature/profile refusal has an empty response body"
            );
            assert_eq!(
                counts(&f).await,
                before,
                "refusal reserved or wrote nothing"
            );
        } else {
            let answer: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                answer.get("result"),
                Some(&json!("records")),
                "an audience array naming admin admits the read"
            );
        }
    }
    f.close().await;
}
