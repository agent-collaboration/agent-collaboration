//! B2c/B2d edge cases on workerd, on their own organization's Fleet object,
//! through central-link with real signed person tokens and proofs:
//! - a request held past exactly one signed deadline (token exp, session or
//!   snapshot), released at it or one millisecond before;
//! - authorization rechecked before an existing operation is replayed;
//! - a changed actor on an existing operation;
//! - a body changed under its signed intent;
//! - an audience array without central's admin audience.
//!
//! Every refusal is judged with the organization's rows before and after it.
use super::link_client::{AdminPerson, Caller};
use super::organization_stamps::{self as stamps, NOW_SECONDS, ProofKey};
use super::record_checks::require;
use super::{
    ADMIN_MEMBER, ADMIN_MEMBER_SUBJECT, ADMIN_OWNER, ADMIN_POLICY, ADMIN_SUBJECT, control,
};
use collaboration_central_conformance::{
    ConformanceResult, DispatchCallSelector, DispatchKey, RuntimeControl,
};
use serde_json::{Value, json};

/// Provisioned only for these cases.
const EDGE: &str = "019f0000-0000-7000-8000-00000000b3d0";
const CREATE: &str = "/organization-admin/v1/create-workspace";
const REVISIONS: &str = "/link/internal/organization-authorization-revisions";
/// Thirty seconds after the fixture time: inside the request, proof and
/// second-factor windows, so only the deadline under test can refuse.
const BOUND_MS: i64 = (NOW_SECONDS + 30) * 1000;

fn intent(operation: &str, name: &str) -> Value {
    json!({"v":1,"operation":"CreateWorkspace","organization":EDGE,"target":EDGE,
        "operationId":operation,"params":{"name":name,"description":""}})
}
fn body(operation: &str, name: &str) -> Value {
    json!({"organizationId":EDGE,"operationId":operation,"name":name,"description":""})
}

fn person(
    id: &'static str,
    subject: &'static str,
    permissions: Vec<&'static str>,
) -> ConformanceResult<AdminPerson> {
    Ok(AdminPerson {
        organization_revision: 1,
        id,
        subject,
        organization: EDGE.to_owned(),
        permissions,
        key: ProofKey::generate()?,
    })
}

/// Workspaces, change records, used-request markers and decisions.
fn rows(observed: &Value) -> [usize; 4] {
    ["workspaces", "records", "usedRequests", "decisions"].map(|field| {
        observed
            .get(field)
            .and_then(Value::as_array)
            .map_or(usize::MAX, Vec::len)
    })
}

/// One signed deadline under test: its claims, the operations released exactly
/// at it and one millisecond before it, and their proofs' serials.
struct DeadlineCase {
    bound: &'static str,
    adjust: fn(&mut Value),
    at_bound: &'static str,
    before_bound: &'static str,
    serial: u16,
}

fn set(claims: &mut Value, name: &str, value: Value) {
    if let Some(object) = claims.as_object_mut() {
        object.insert(name.to_owned(), value);
    }
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

async fn set_time_millis(caller: &Caller, millis: i64) -> ConformanceResult<()> {
    control(
        caller,
        EDGE,
        RuntimeControl::SetTime {
            now: collaboration_central_core::RegistryTime(
                chrono::DateTime::from_timestamp_millis(millis).ok_or("fixture time")?,
            ),
        },
    )
    .await
}

/// Hold a verified create before its transaction, move the Fleet's clock to
/// `release_millis`, then release it. The clock returns to the fixture time
/// first, so the token verifies as current.
async fn held_create(
    caller: &Caller,
    owner: AdminPerson,
    operation: &'static str,
    adjust: fn(&mut Value),
    serial: u16,
    release_millis: i64,
) -> ConformanceResult<(u16, Value)> {
    set_time_millis(caller, NOW_SECONDS * 1000).await?;
    let key = DispatchKey::new(
        collaboration_central_core::ObjectAddress::Fleet {
            organization: EDGE.to_owned().try_into()?,
        },
        DispatchCallSelector::OrganizationAdmin,
    );
    control(
        caller,
        EDGE,
        RuntimeControl::HoldNextDispatch { key: key.clone() },
    )
    .await?;
    let held_caller = caller.clone();
    let held = tokio::spawn(async move {
        let sent = async {
            let token = owner.token_with(&adjust)?;
            held_caller
                .admin_with(
                    CREATE,
                    &intent(operation, operation),
                    &body(operation, operation),
                    &token,
                    &owner.key,
                    serial,
                )
                .await
        };
        sent.await.map_err(|error| error.to_string())
    });
    // Returns once the request verified and stopped before its transaction.
    control(
        caller,
        EDGE,
        RuntimeControl::AwaitHeldDispatch { key: key.clone() },
    )
    .await?;
    set_time_millis(caller, release_millis).await?;
    control(caller, EDGE, RuntimeControl::ResumeDispatch { key }).await?;
    Ok(held.await??)
}

async fn create(
    caller: &Caller,
    who: &AdminPerson,
    operation: &str,
    name: &str,
    serial: u16,
    adjust: &dyn Fn(&mut Value),
) -> ConformanceResult<(u16, Value)> {
    caller
        .admin_with(
            CREATE,
            &intent(operation, name),
            &body(operation, name),
            &who.token_with(adjust)?,
            &who.key,
            serial,
        )
        .await
}

pub(crate) async fn run(caller: &Caller) -> ConformanceResult<()> {
    let provisioned = caller.provision(EDGE, &stamps::stamp(EDGE)?).await?;
    let mut floors = vec![
        caller
            .post(
                REVISIONS,
                &json!({"kind":"organization","organizationId":EDGE,"revision":1,"state":"active",
                    "permissionPolicyDigest":ADMIN_POLICY}),
            )
            .await?
            .0,
    ];
    for (who, subject) in [
        (ADMIN_OWNER, ADMIN_SUBJECT),
        (ADMIN_MEMBER, ADMIN_MEMBER_SUBJECT),
    ] {
        floors.push(
            caller
                .post(
                    REVISIONS,
                    &json!({"kind":"person","organizationId":EDGE,"personId":who,
                        "subjectId":subject,"revision":1,"state":"active"}),
                )
                .await?
                .0,
        );
    }
    require(
        provisioned == 200 && floors == vec![200, 200, 200],
        "the edge-case organization is stamped and its floors are pushed",
        &(provisioned, floors),
    )?;

    // 1 and 6: each signed deadline rechecked at the held request's applying clock.
    let cases = [
        DeadlineCase {
            bound: "token exp",
            adjust: token_bound,
            at_bound: "019f0000-0000-7000-8000-00000000b3e1",
            before_bound: "019f0000-0000-7000-8000-00000000b3e2",
            serial: 700,
        },
        DeadlineCase {
            bound: "session deadline",
            adjust: session_bound,
            at_bound: "019f0000-0000-7000-8000-00000000b3e3",
            before_bound: "019f0000-0000-7000-8000-00000000b3e4",
            serial: 702,
        },
        DeadlineCase {
            bound: "snapshot deadline",
            adjust: snapshot_bound,
            at_bound: "019f0000-0000-7000-8000-00000000b3e5",
            before_bound: "019f0000-0000-7000-8000-00000000b3e6",
            serial: 704,
        },
    ];
    let owner_permissions = || vec!["organization:view", "workspace:create"];
    let mut held = Vec::new();
    for DeadlineCase {
        bound,
        adjust,
        at_bound,
        before_bound,
        serial,
    } in cases
    {
        let before = rows(&caller.records(EDGE).await?);
        let refused = held_create(
            caller,
            person(ADMIN_OWNER, ADMIN_SUBJECT, owner_permissions())?,
            at_bound,
            adjust,
            serial,
            BOUND_MS,
        )
        .await?;
        let after_refusal = rows(&caller.records(EDGE).await?);
        let applied = held_create(
            caller,
            person(ADMIN_OWNER, ADMIN_SUBJECT, owner_permissions())?,
            before_bound,
            adjust,
            serial + 1,
            BOUND_MS - 1,
        )
        .await?;
        let after_applied = rows(&caller.records(EDGE).await?);
        held.push((
            bound,
            refused,
            before == after_refusal,
            applied,
            after_applied[0] == before[0] + 1,
        ));
    }
    set_time_millis(caller, NOW_SECONDS * 1000).await?;
    require(
        held.iter()
            .all(|(_, refused, unchanged, applied, created)| {
                refused.1 == json!({"result":"refused","reason":"expired"})
                    && *unchanged
                    && applied.1.pointer("/result") == Some(&json!("applied"))
                    && *created
            }),
        "a held request is refused at each signed deadline with no marker or write, and applies one millisecond before",
        &held,
    )?;

    // 3: a different authorized person reusing an existing operation.
    let owner = person(ADMIN_OWNER, ADMIN_SUBJECT, owner_permissions())?;
    let first = "019f0000-0000-7000-8000-00000000b3e7";
    let created = create(caller, &owner, first, "first actor", 710, &|_| {}).await?;
    let before_other = rows(&caller.records(EDGE).await?);
    let other = person(ADMIN_MEMBER, ADMIN_MEMBER_SUBJECT, vec!["workspace:create"])?;
    let by_other = create(caller, &other, first, "first actor", 711, &|_| {}).await?;
    let after_other = rows(&caller.records(EDGE).await?);
    require(
        created.1.pointer("/result") == Some(&json!("applied"))
            && by_other.1 == json!({"result":"refused","reason":"conflict"})
            && after_other[0] == before_other[0]
            && after_other[1] == before_other[1],
        "a different authorized person reusing an operation gets the conflict, never the first answer",
        &(created, by_other, before_other, after_other),
    )?;

    // 4: a body changed under its signed intent.
    let tampered_operation = "019f0000-0000-7000-8000-00000000b3e8";
    let before_tamper = rows(&caller.records(EDGE).await?);
    let tampered = caller
        .admin_with(
            CREATE,
            &intent(tampered_operation, "signed name"),
            &body(tampered_operation, "changed name"),
            &owner.token()?,
            &owner.key,
            712,
        )
        .await?;
    let after_tamper = rows(&caller.records(EDGE).await?);
    let signed = create(
        caller,
        &owner,
        tampered_operation,
        "signed name",
        713,
        &|_| {},
    )
    .await?;
    require(
        tampered.0 == 401
            && before_tamper == after_tamper
            && signed.1.pointer("/result") == Some(&json!("applied")),
        "a body changed under its signed intent is refused at verification; the signed body applies",
        &(tampered, before_tamper, after_tamper, signed),
    )?;

    // 5: an audience array that does not name the admin audience.
    let before_audience = rows(&caller.records(EDGE).await?);
    let missing = create(
        caller,
        &owner,
        "019f0000-0000-7000-8000-00000000b3e9",
        "missing audience",
        714,
        &|claims| {
            set(
                claims,
                "aud",
                json!(["https://other.example.invalid/admin"]),
            )
        },
    )
    .await?;
    let after_audience = rows(&caller.records(EDGE).await?);
    let named = create(
        caller,
        &owner,
        "019f0000-0000-7000-8000-00000000b3ea",
        "named audience",
        715,
        &|claims| {
            set(
                claims,
                "aud",
                json!([
                    "https://other.example.invalid/admin",
                    stamps::ADMIN_AUDIENCE
                ]),
            );
        },
    )
    .await?;
    require(
        missing.0 == 401
            && before_audience == after_audience
            && named.1.pointer("/result") == Some(&json!("applied")),
        "an audience array without the admin audience is refused; one that names it applies",
        &(missing, before_audience, after_audience, named),
    )?;

    // 2: authorization rechecked before an existing operation is replayed.
    // Last, because the owner's revocation does not come back.
    let replayed = "019f0000-0000-7000-8000-00000000b3eb";
    let original = create(caller, &owner, replayed, "replayed", 716, &|_| {}).await?;
    let before_replay = rows(&caller.records(EDGE).await?);
    let member = person(
        ADMIN_MEMBER,
        ADMIN_MEMBER_SUBJECT,
        vec!["organization:view"],
    )?;
    let by_member = create(caller, &member, replayed, "replayed", 717, &|_| {}).await?;
    let after_member = rows(&caller.records(EDGE).await?);
    let revoked = caller
        .post(
            REVISIONS,
            &json!({"kind":"person","organizationId":EDGE,"personId":ADMIN_OWNER,
                "subjectId":ADMIN_SUBJECT,"revision":2,"state":"inactive"}),
        )
        .await?;
    let by_revoked = create(caller, &owner, replayed, "replayed", 718, &|_| {}).await?;
    let after_revoked = rows(&caller.records(EDGE).await?);
    require(
        original.1.pointer("/result") == Some(&json!("applied"))
            && by_member.1 == json!({"result":"refused","reason":"notPermitted"})
            && after_member == before_replay
            && revoked.0 == 200
            && by_revoked.1 == json!({"result":"refused","reason":"notPermitted"})
            && after_revoked == before_replay,
        "an existing operation is refused, not replayed, to a member or to an owner revoked since",
        &(
            original,
            by_member,
            revoked.0,
            by_revoked,
            before_replay,
            after_revoked,
        ),
    )
}
