//! B2c/B2d edge cases on Tokio, each through the real link socket with real
//! signed person tokens and proofs: deadlines rechecked at a held request's
//! applying clock, authorization rechecked before an existing operation is
//! replayed, a changed actor on an existing operation, an unsigned body change
//! and an audience array that does not name central's admin audience. Every
//! refusal is judged with the rows before and after it.
#![cfg(feature = "conformance")]
use crate::organization_admin_link::{
    CREATE, MEMBER, MEMBER_SUBJECT, ORGANIZATION, OWNER, OWNER_SUBJECT, PUSH, Person, answer,
    create_body, create_intent, fresh_request_id, person_push, seed_floors, start,
};
use crate::organization_provisioning_link::control;
use crate::organization_stamps::{
    self as stamps, ADMIN_AUDIENCE, NOW_SECONDS, ProofKey, SignInKeys, link_post,
};
use collaboration_central_conformance::{
    ConformanceResult, DispatchCallSelector, DispatchKey, JwksFixtureServer, RuntimeControl,
};
use collaboration_central_core::ObjectAddress;
use collaboration_central_tokio::TokioServer;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// What the organization's Fleet holds: workspaces, change records, used
/// request markers and request decisions, read by the owner-only observer.
#[derive(Debug, Eq, PartialEq)]
struct Rows {
    workspaces: usize,
    records: usize,
    markers: usize,
    decisions: usize,
}

async fn rows(server: &TokioServer) -> ConformanceResult<Rows> {
    let observed: Value = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!(
            "{}{}",
            server.base_url(),
            collaboration_central_core::ORGANIZATION_RECORDS_PATH
        ))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&json!({"organization":ORGANIZATION}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let count = |field: &str| -> ConformanceResult<usize> {
        Ok(observed
            .get(field)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{field} is not a list"))?
            .len())
    };
    Ok(Rows {
        workspaces: count("workspaces")?,
        records: count("records")?,
        markers: count("usedRequests")?,
        decisions: count("decisions")?,
    })
}

/// A stamped organization with floors, served by a fresh central.
async fn ready(
    root: &Path,
    access: &JwksFixtureServer,
    keys: &SignInKeys,
) -> ConformanceResult<(TokioServer, PathBuf)> {
    let (server, socket) = start(root, access, keys).await?;
    let provisioned = link_post(
        &socket,
        "/internal/organization-provisioning",
        &json!({"organizationId":ORGANIZATION,"organizationStatement":stamps::stamp(ORGANIZATION)?}),
    )
    .await?;
    if provisioned.0 != 200 {
        return Err(format!("provisioning answered {}", provisioned.0).into());
    }
    server.shutdown().await?;
    seed_floors(root).await?;
    start(root, access, keys).await
}

fn owner(permissions: Vec<&'static str>) -> ConformanceResult<Person> {
    Ok(Person {
        id: OWNER,
        subject: OWNER_SUBJECT,
        revision: 4,
        permissions,
        second_factor_at: NOW_SECONDS - 60,
        key: ProofKey::generate()?,
    })
}

fn member(permissions: Vec<&'static str>) -> ConformanceResult<Person> {
    Ok(Person {
        id: MEMBER,
        subject: MEMBER_SUBJECT,
        revision: 1,
        permissions,
        second_factor_at: NOW_SECONDS - 60,
        key: ProofKey::generate()?,
    })
}

/// One create of `operation` named `name` by `person`, with a fresh proof.
async fn create(
    socket: &Path,
    person: &Person,
    operation: &str,
    name: &str,
    adjust: &dyn Fn(&mut Value),
) -> ConformanceResult<(u16, Value)> {
    answer(
        socket,
        CREATE,
        &person.headers_with(
            CREATE,
            &create_intent(operation, name),
            &fresh_request_id(),
            adjust,
        )?,
        &create_body(operation, name),
    )
    .await
}

fn time_millis(millis: i64) -> ConformanceResult<collaboration_central_core::RegistryTime> {
    Ok(collaboration_central_core::RegistryTime(
        chrono::DateTime::from_timestamp_millis(millis).ok_or("fixture time")?,
    ))
}

/// Hold a verified create before its transaction, move the Fleet clock to
/// `release_millis`, then release it. The clock returns to the fixture time
/// first, so the token verifies as current.
async fn held_create(
    server: &TokioServer,
    socket: &Path,
    person: Person,
    operation: &'static str,
    adjust: fn(&mut Value),
    release_millis: i64,
) -> ConformanceResult<(u16, Value)> {
    let base = server.base_url().to_owned();
    control(
        &base,
        ORGANIZATION,
        RuntimeControl::SetTime {
            now: time_millis(NOW_SECONDS * 1000)?,
        },
    )
    .await?;
    let key = DispatchKey::new(
        ObjectAddress::Fleet {
            organization: ORGANIZATION.to_owned().try_into()?,
        },
        DispatchCallSelector::OrganizationAdmin,
    );
    control(
        &base,
        ORGANIZATION,
        RuntimeControl::HoldNextDispatch { key: key.clone() },
    )
    .await?;
    let held_socket = socket.to_owned();
    let headers = person.headers_with(
        CREATE,
        &create_intent(operation, operation),
        &fresh_request_id(),
        &adjust,
    )?;
    let held = tokio::spawn(async move {
        answer(
            &held_socket,
            CREATE,
            &headers,
            &create_body(operation, operation),
        )
        .await
        .map_err(|error| error.to_string())
    });
    // Returns once the request verified and stopped before its transaction.
    control(
        &base,
        ORGANIZATION,
        RuntimeControl::AwaitHeldDispatch { key: key.clone() },
    )
    .await?;
    control(
        &base,
        ORGANIZATION,
        RuntimeControl::SetTime {
            now: time_millis(release_millis)?,
        },
    )
    .await?;
    control(&base, ORGANIZATION, RuntimeControl::ResumeDispatch { key }).await?;
    Ok(held.await??)
}

/// Thirty seconds after the fixture time: inside the request and proof
/// windows and the second-factor window, so only the deadline under test can
/// refuse.
const BOUND_MS: i64 = (NOW_SECONDS + 30) * 1000;

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
/// One signed deadline under test: its claims, and the operations released
/// exactly at it and one millisecond before it.
struct DeadlineCase {
    bound: &'static str,
    adjust: fn(&mut Value),
    at_bound: &'static str,
    before_bound: &'static str,
}

fn set(claims: &mut Value, name: &str, value: Value) {
    if let Some(object) = claims.as_object_mut() {
        object.insert(name.to_owned(), value);
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare each held answer with the rows before and after it while fixture errors propagate."
)]
#[tokio::test]
async fn a_held_request_is_refused_at_each_signed_deadline_and_applied_just_before()
-> ConformanceResult<()> {
    // Wrong builds caught: a deadline checked only when the token verified
    // (the held request applies at its deadline); the applying clock sampled
    // before the hold; one of the three signed deadlines ignored; a refusal
    // that still reserves a marker or records a decision.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let (server, socket) = ready(root.path(), &access, &keys).await?;
    let cases = [
        DeadlineCase {
            bound: "token exp",
            adjust: token_bound,
            at_bound: "019f0000-0000-7000-8000-00000000d401",
            before_bound: "019f0000-0000-7000-8000-00000000d402",
        },
        DeadlineCase {
            bound: "session deadline",
            adjust: session_bound,
            at_bound: "019f0000-0000-7000-8000-00000000d403",
            before_bound: "019f0000-0000-7000-8000-00000000d404",
        },
        DeadlineCase {
            bound: "snapshot deadline",
            adjust: snapshot_bound,
            at_bound: "019f0000-0000-7000-8000-00000000d405",
            before_bound: "019f0000-0000-7000-8000-00000000d406",
        },
    ];
    let mut observed = Vec::new();
    for DeadlineCase {
        bound,
        adjust,
        at_bound,
        before_bound,
    } in cases
    {
        let before = rows(&server).await?;
        let refused = held_create(
            &server,
            &socket,
            owner(vec!["workspace:create"])?,
            at_bound,
            adjust,
            BOUND_MS,
        )
        .await?;
        let after_refusal = rows(&server).await?;
        let applied = held_create(
            &server,
            &socket,
            owner(vec!["workspace:create"])?,
            before_bound,
            adjust,
            BOUND_MS - 1,
        )
        .await?;
        let after_applied = rows(&server).await?;
        observed.push((
            bound,
            before,
            refused,
            after_refusal,
            applied,
            after_applied,
        ));
    }
    server.shutdown().await?;

    for (bound, before, refused, after_refusal, applied, after_applied) in observed {
        assert_eq!(
            refused.1,
            json!({"result":"refused","reason":"expired"}),
            "{bound}: released at the deadline into its transaction"
        );
        assert_eq!(
            after_refusal, before,
            "{bound}: the refusal reserved, decided or wrote nothing"
        );
        assert_eq!(
            applied.1.pointer("/result"),
            Some(&json!("applied")),
            "{bound}: released one millisecond before the deadline: {applied:?}"
        );
        assert_eq!(
            after_applied.workspaces,
            before.workspaces + 1,
            "{bound}: the control created its workspace"
        );
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare each answer with the rows before and after it while fixture errors propagate."
)]
#[tokio::test]
async fn an_existing_operation_is_not_replayed_to_a_caller_not_authorized_now()
-> ConformanceResult<()> {
    // Wrong builds caught: the ledger's earlier answer replayed before
    // admission, so a member without the permission, or an owner revoked since,
    // receives the cached workspace.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let (server, socket) = ready(root.path(), &access, &keys).await?;
    let operation = "019f0000-0000-7000-8000-00000000d411";
    let created = create(
        &socket,
        &owner(vec!["workspace:create"])?,
        operation,
        "replayed",
        &|_| {},
    )
    .await?;
    let before = rows(&server).await?;
    let by_member = create(
        &socket,
        &member(vec!["organization:view"])?,
        operation,
        "replayed",
        &|_| {},
    )
    .await?;
    let after_member = rows(&server).await?;
    let revoked = link_post(&socket, PUSH, &person_push(5, "inactive", OWNER_SUBJECT)).await?;
    let by_revoked_owner = create(
        &socket,
        &owner(vec!["workspace:create"])?,
        operation,
        "replayed",
        &|_| {},
    )
    .await?;
    let after_owner = rows(&server).await?;
    server.shutdown().await?;

    assert_eq!(created.1.pointer("/result"), Some(&json!("applied")));
    assert_eq!(
        by_member.1,
        json!({"result":"refused","reason":"notPermitted"}),
        "a member repeating an existing operation is refused, not replayed"
    );
    assert_eq!(after_member, before, "the member's refusal changed nothing");
    assert_eq!(revoked.0, 200, "the owner's revocation is acknowledged");
    assert_eq!(
        by_revoked_owner.1,
        json!({"result":"refused","reason":"notPermitted"}),
        "the owner, revoked since, repeating the operation is refused, not replayed"
    );
    assert_eq!(
        after_owner, before,
        "the revoked owner's refusal changed nothing"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare each answer with the rows before and after it while fixture errors propagate."
)]
#[tokio::test]
async fn another_authorized_person_reusing_an_operation_gets_a_conflict() -> ConformanceResult<()> {
    // Wrong build caught: an existing operation answered to whoever repeats it,
    // so a second authorized person receives the first actor's workspace.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let (server, socket) = ready(root.path(), &access, &keys).await?;
    let operation = "019f0000-0000-7000-8000-00000000d421";
    let created = create(
        &socket,
        &owner(vec!["workspace:create"])?,
        operation,
        "first actor",
        &|_| {},
    )
    .await?;
    let before = rows(&server).await?;
    let by_other = create(
        &socket,
        &member(vec!["workspace:create"])?,
        operation,
        "first actor",
        &|_| {},
    )
    .await?;
    let after = rows(&server).await?;
    server.shutdown().await?;

    assert_eq!(created.1.pointer("/result"), Some(&json!("applied")));
    assert_eq!(
        by_other.1,
        json!({"result":"refused","reason":"conflict"}),
        "a different authorized actor gets the conflict, never the first answer"
    );
    assert_eq!(after.workspaces, before.workspaces, "no second workspace");
    assert_eq!(after.records, before.records, "no second change record");
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare each answer with the rows before and after it while fixture errors propagate."
)]
#[tokio::test]
async fn a_body_changed_under_its_signed_intent_is_refused_before_admission()
-> ConformanceResult<()> {
    // Wrong build caught: the body applied without matching the intent the
    // proof signed. The refusal comes from verification (401), not the ledger.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let (server, socket) = ready(root.path(), &access, &keys).await?;
    let person = owner(vec!["workspace:create"])?;
    let operation = "019f0000-0000-7000-8000-00000000d431";
    let before = rows(&server).await?;
    let tampered = answer(
        &socket,
        CREATE,
        &person.headers(
            CREATE,
            &create_intent(operation, "signed name"),
            &fresh_request_id(),
        )?,
        &create_body(operation, "changed name"),
    )
    .await?;
    let after = rows(&server).await?;
    let signed = create(&socket, &person, operation, "signed name", &|_| {}).await?;
    server.shutdown().await?;

    assert_eq!(
        tampered.0, 401,
        "a body that differs from the signed intent is refused at verification: {tampered:?}"
    );
    assert_eq!(
        after, before,
        "the tampered request reserved or wrote nothing"
    );
    assert_eq!(
        signed.1.pointer("/result"),
        Some(&json!("applied")),
        "the signed body applies"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare each answer with the rows before and after it while fixture errors propagate."
)]
#[tokio::test]
async fn a_person_audience_array_without_the_admin_audience_is_refused() -> ConformanceResult<()> {
    // Wrong build caught: a person token admitted whatever its audience array
    // names. The control's array includes the admin audience and applies.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let (server, socket) = ready(root.path(), &access, &keys).await?;
    let person = owner(vec!["workspace:create"])?;
    let before = rows(&server).await?;
    let missing = create(
        &socket,
        &person,
        "019f0000-0000-7000-8000-00000000d441",
        "missing audience",
        &|claims| {
            set(
                claims,
                "aud",
                json!(["https://other.example.invalid/admin"]),
            )
        },
    )
    .await?;
    let after = rows(&server).await?;
    let named = create(
        &socket,
        &person,
        "019f0000-0000-7000-8000-00000000d442",
        "named audience",
        &|claims| {
            set(
                claims,
                "aud",
                json!(["https://other.example.invalid/admin", ADMIN_AUDIENCE]),
            );
        },
    )
    .await?;
    server.shutdown().await?;

    assert_eq!(
        missing.0, 401,
        "an audience array without the admin audience is refused: {missing:?}"
    );
    assert_eq!(
        after, before,
        "the refused request reserved or wrote nothing"
    );
    assert_eq!(
        named.1.pointer("/result"),
        Some(&json!("applied")),
        "an array that names the admin audience applies: {named:?}"
    );
    Ok(())
}
