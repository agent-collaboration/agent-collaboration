//! Independent admission boundaries from B2's held-create cases. These read-route
//! cases do not close the held workspace effect or operation replay assertions.
use super::support::*;
use central_storage::SqlValue;
use collaboration_central_core::PausePoint;
use serde_json::{Value, json};
use std::time::Duration;

const BOUND_MS: i64 = (signed::NOW_SECONDS + 30) * 1000;
fn set(claims: &mut Value, name: &str, value: Value) {
    if let Some(object) = claims.as_object_mut() {
        object.insert(name.to_owned(), value);
    }
}
fn token_bound(claims: &mut Value) {
    set(claims, "exp", json!(signed::NOW_SECONDS + 30));
    set(
        claims,
        "session_expires_at_ms",
        json!((signed::NOW_SECONDS + 60) * 1000),
    );
    set(
        claims,
        "authorization_snapshot_valid_until_ms",
        json!((signed::NOW_SECONDS + 90) * 1000),
    );
}
fn session_bound(claims: &mut Value) {
    set(claims, "exp", json!(signed::NOW_SECONDS + 90));
    set(claims, "session_expires_at_ms", json!(BOUND_MS));
    set(
        claims,
        "authorization_snapshot_valid_until_ms",
        json!((signed::NOW_SECONDS + 60) * 1000),
    );
}
fn snapshot_bound(claims: &mut Value) {
    set(claims, "exp", json!(signed::NOW_SECONDS + 90));
    set(
        claims,
        "session_expires_at_ms",
        json!((signed::NOW_SECONDS + 60) * 1000),
    );
    set(
        claims,
        "authorization_snapshot_valid_until_ms",
        json!(BOUND_MS),
    );
}
async fn counts(f: &Fixture) -> Result<Vec<Vec<SqlValue>>> {
    f.rows(A,"SELECT (SELECT COUNT(*) FROM workspaces),(SELECT COUNT(*) FROM organization_change_records),(SELECT COUNT(*) FROM organization_used_requests),(SELECT COUNT(*) FROM request_log)",vec![]).await
}
async fn held_read(f: &Fixture, adjust: fn(&mut Value), release_after_ms: u64) -> Result<Reply> {
    let person = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let body = serde_json::to_string(&json!({"organizationId":A,"after":null,"limit":100}))?;
    let headers = person.headers_with(
        signed::RECORDS,
        &signed::records_intent(A, None, 100),
        &signed::fresh_request_id(signed::NOW_SECONDS)?,
        signed::NOW_SECONDS,
        &adjust,
    )?;
    let runtime = f.runtime(A).await?;
    let mut held = runtime
        .pause_points()
        .hold_central(PausePoint::BeforeSignInSample);
    let client = f.client();
    let request = tokio::spawn(async move { post(&client, signed::RECORDS, body, headers).await });
    held.until_held().await;
    f.clock.advance(Duration::from_millis(release_after_ms))?;
    held.release();
    request.await?
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Literal B2 deadline refusals and unchanged-row assertions exercise independent read admission."
)]
#[tokio::test]
async fn each_signed_deadline_refuses_at_the_bound_and_admits_a_read_one_millisecond_before()
-> Result {
    type ClaimsAdjustment = fn(&mut Value);
    let cases: [(&str, ClaimsAdjustment); 3] = [
        ("token exp", token_bound),
        ("session deadline", session_bound),
        ("snapshot deadline", snapshot_bound),
    ];
    for (bound, adjust) in cases {
        for (released_after, refused) in [(30_000, true), (29_999, false)] {
            let f = Fixture::new().await?;
            f.ready().await?;
            let before = counts(&f).await?;
            let answer = held_read(&f, adjust, released_after).await?;
            let after = counts(&f).await?;
            assert_eq!(answer.status, 200);
            if refused {
                assert_eq!(
                    answer.value()?,
                    json!({"result":"refused","reason":"expired"}),
                    "{bound}: released at the deadline into its applying turn"
                );
                assert_eq!(
                    after, before,
                    "{bound}: the refusal reserved, decided or wrote nothing"
                );
            } else {
                assert_eq!(
                    answer.value()?.get("result"),
                    Some(&json!("records")),
                    "{bound}: released one millisecond before the deadline"
                );
                assert_eq!(
                    after,
                    vec![vec![number(0)?, number(1)?, number(1)?, number(0)?]],
                    "the admitted read reserves one proof, with no held workspace effect"
                );
            }
            f.close().await?;
        }
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Request and proof age are checked on the applying sample with the original strict lower bound."
)]
#[tokio::test]
async fn applying_request_window_refuses_exactly_sixty_five_seconds_old_and_admits_before() -> Result
{
    // Fixture proof iat and UUIDv7 jti are both one second before NOW.
    for (release_after, refused) in [(64_000, true), (63_999, false)] {
        let f = Fixture::new().await?;
        f.ready().await?;
        let before = counts(&f).await?;
        let answer = held_read(&f, |_| {}, release_after).await?;
        assert_eq!(answer.status, 200);
        if refused {
            assert_eq!(
                answer.value()?,
                json!({"result":"refused","reason":"requestTime"})
            );
            assert_eq!(
                counts(&f).await?,
                before,
                "out-of-window proof must reserve nothing"
            );
        } else {
            assert_eq!(answer.value()?.get("result"), Some(&json!("records")));
        }
        f.close().await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Independent floor, subject and policy checks fail before writing a fresh-proof marker."
)]
#[tokio::test]
async fn missing_floors_wrong_subject_policy_and_stopped_or_deleted_org_refuse_reads() -> Result {
    for (case, expected) in [
        ("organization missing", "authorizationSyncRequired"),
        ("person missing", "authorizationSyncRequired"),
        ("wrong subject", "notPermitted"),
        ("policy changed", "refreshRequired"),
        ("stopped", "notPermitted"),
        ("deleted", "notPermitted"),
    ] {
        let f = Fixture::new().await?;
        if case == "organization missing" {
            assert_eq!(
                f.provision(A, &signed::stamp(A, signed::NOW_SECONDS)?)
                    .await?
                    .status,
                200
            );
        } else if case == "person missing" {
            assert_eq!(
                f.provision(A, &signed::stamp(A, signed::NOW_SECONDS)?)
                    .await?
                    .status,
                200
            );
            assert_eq!(
                f.post(REVISIONS, &organization_floor(A, 3, "active"))
                    .await?
                    .status,
                200
            );
        } else {
            f.ready().await?;
        }
        let mut person = signed::Person::owner(A, signed::NOW_SECONDS)?;
        match case {
            "wrong subject" => person.subject = signed::MEMBER_SUBJECT.into(),
            "policy changed" => {
                person.policy_digest =
                    "0000000000000000000000000000000000000000000000000000000000000000".into()
            }
            "stopped" | "deleted" => {
                assert_eq!(
                    f.post(REVISIONS, &organization_floor(A, 4, case))
                        .await?
                        .status,
                    200
                );
                person.organization_revision = 4;
            }
            _ => {}
        }
        let before = counts(&f).await?;
        let answer = f
            .person(
                signed::RECORDS,
                &json!({"organizationId":A,"after":null,"limit":100}),
                &person,
                &signed::records_intent(A, None, 100),
                &signed::fresh_request_id(signed::NOW_SECONDS)?,
            )
            .await?;
        assert_eq!(answer.status, 200);
        assert_eq!(
            answer.value()?,
            json!({"result":"refused","reason":expected}),
            "{case}"
        );
        assert_eq!(
            counts(&f).await?,
            before,
            "{case}: refusal changed no business, proof or decision rows"
        );
        f.close().await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The audience-array control preserves the original rejection and acceptance oracles on a read route."
)]
#[tokio::test]
async fn audience_array_must_name_admin_and_a_refusal_reserves_nothing() -> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let person = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let before = counts(&f).await?;
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
        let headers = person.headers_with(
            signed::RECORDS,
            &signed::records_intent(A, None, 100),
            &signed::fresh_request_id(signed::NOW_SECONDS)?,
            signed::NOW_SECONDS,
            &|claims| set(claims, "aud", audience.clone()),
        )?;
        let answer = post(
            &f.client(),
            signed::RECORDS,
            serde_json::to_string(&json!({"organizationId":A,"after":null,"limit":100}))?,
            headers,
        )
        .await?;
        assert_eq!(answer.status, status);
        if status == 401 {
            assert!(answer.body.is_empty());
            assert_eq!(
                counts(&f).await?,
                before,
                "the refused request reserved or wrote nothing"
            );
        } else {
            assert_eq!(
                answer.value()?.get("result"),
                Some(&json!("records")),
                "an array that names the admin audience admits the read"
            );
        }
    }
    f.close().await
}
