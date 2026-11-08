//! B2c/B2d on Tokio: a person's DPoP-bound admin requests over the real link
//! socket, admitted against seeded floors, applied through the Fleet's own
//! decision path, with every refusal leaving the stores unchanged.
#![cfg(feature = "conformance")]
use crate::configuration;
use crate::organization_stamps::{
    self as stamps, ADMIN_AUDIENCE, AUDIENCE, ISSUER, NOW_SECONDS, PUBLIC_ORIGIN, ProofKey,
    SignInKeys, link_post, link_post_with,
};
use collaboration_central_conformance::{
    ConformanceResult, JwksFixtureServer, RuntimeControl, RuntimeControlRequest, fixture_policy,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{SignInLinkSettings, TokioServer, TokioSettings};
use serde_json::{Value, json};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query_scalar::query_scalar,
};
use std::{
    path::Path,
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000d201";
const OWNER: &str = "019f0000-0000-7000-8000-00000000d202";
const OWNER_SUBJECT: &str = "019f0000-0000-7000-8000-00000000d203";
const MEMBER: &str = "019f0000-0000-7000-8000-00000000d204";
const MEMBER_SUBJECT: &str = "019f0000-0000-7000-8000-00000000d205";
const SESSION: &str = "019f0000-0000-7000-8000-00000000d206";
/// The sign-in service's digest of its permission policy (shared vector).
const POLICY: &str = "2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22";
const CREATE: &str = "/organization-admin/v1/create-workspace";
const LOOKUP: &str = "/organization-admin/v1/workspace-lookup";
const RECORDS: &str = "/organization-admin/v1/change-records";

fn settings(root: &Path, jwks: &str) -> ConformanceResult<TokioSettings> {
    Ok(TokioSettings {
        legacy_organization: collaboration_central_conformance::legacy_organization(),
        root: root.into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.into(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
}

async fn start(
    root: &Path,
    access: &JwksFixtureServer,
    keys: &SignInKeys,
) -> ConformanceResult<(TokioServer, std::path::PathBuf)> {
    let socket = root.join("sign-in-link.sock");
    let server = TokioServer::start_with_sign_in_and_controls(
        settings(root, &access.url)?,
        SignInLinkSettings {
            issuer: ISSUER.into(),
            audience: AUDIENCE.into(),
            admin_audience: ADMIN_AUDIENCE.into(),
            public_origin: PUBLIC_ORIGIN.into(),
            jwks_url: keys.url.clone(),
            socket: socket.clone(),
            service_id: "019f0000-0000-7000-8000-00000000e501"
                .to_owned()
                .try_into()?,
        },
        vec![RuntimeControlRequest {
            address: ObjectAddress::Fleet {
                organization: ORGANIZATION.to_owned().try_into()?,
            },
            command: RuntimeControl::SetTime {
                now: RegistryTime(
                    chrono::DateTime::from_timestamp(NOW_SECONDS, 0).ok_or("fixture time")?,
                ),
            },
        }],
    )
    .await?;
    Ok((server, socket))
}

/// Floors are what the sign-in service would have pushed; seeded only here.
async fn seed_floors(root: &Path) -> ConformanceResult<()> {
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("fleet-{ORGANIZATION}.db")))
        .connect()
        .await?;
    let at = NOW_SECONDS * 1000;
    for statement in [
        format!(
            "INSERT INTO organization_authorization_floors VALUES('{ORGANIZATION}',3,'active','{POLICY}',{},{at},{at})",
            at - 100_000
        ),
        format!(
            "INSERT INTO person_organization_authorization_floors VALUES('{OWNER_SUBJECT}','{ORGANIZATION}','{OWNER}',4,'active',{at},{at})"
        ),
        format!(
            "INSERT INTO person_organization_authorization_floors VALUES('{MEMBER_SUBJECT}','{ORGANIZATION}','{MEMBER}',1,'active',{at},{at})"
        ),
    ] {
        sqlx_core::query::query::<sqlx_turso::Turso>(sqlx_core::sql_str::AssertSqlSafe(statement))
            .execute(&mut store)
            .await?;
    }
    store.close().await?;
    Ok(())
}

struct Person {
    id: &'static str,
    subject: &'static str,
    revision: i64,
    permissions: Vec<&'static str>,
    second_factor_at: i64,
    key: ProofKey,
}

static REQUESTS: AtomicU32 = AtomicU32::new(1);

/// A fresh UUIDv7 a second before the Fleet's pinned time.
fn fresh_request_id() -> String {
    let millis = NOW_SECONDS * 1000 - 1000;
    let serial = REQUESTS.fetch_add(1, Ordering::SeqCst);
    format!(
        "{:08x}-{:04x}-7000-8000-{:012x}",
        millis >> 16,
        millis & 0xffff,
        serial
    )
}

impl Person {
    fn token(&self) -> ConformanceResult<String> {
        let claims = json!({
            "iss":ISSUER,"aud":ADMIN_AUDIENCE,"sub":self.id,"sid":SESSION,
            "iat":NOW_SECONDS - 10,"exp":NOW_SECONDS + 290,"cnf":{"jkt":self.key.thumbprint()?},
            "token_profile":"person-fleet","organization_id":ORGANIZATION,
            "authorization_subject_id":self.subject,"person_revision":self.revision,
            "organization_revision":3,"permission_policy_digest":POLICY,
            "permissions":self.permissions,
            "second_factor_at":self.second_factor_at,"second_factor_method":"totp",
            "session_expires_at_ms":(NOW_SECONDS + 86_400) * 1000,
            "authorization_snapshot_at_ms":NOW_SECONDS * 1000 - 10_000,
            "authorization_snapshot_valid_until_ms":NOW_SECONDS * 1000 + 200_000
        });
        stamps::sign(
            &json!({"typ":"at+jwt","alg":"EdDSA","kid":"stamp-a"}),
            &claims,
            7,
        )
    }

    /// Headers for one attempt: the token, and a proof with a fresh jti.
    fn headers(
        &self,
        path: &str,
        intent: &Value,
        jti: &str,
    ) -> ConformanceResult<Vec<(&'static str, String)>> {
        let token = self.token()?;
        let proof = self.key.proof(&json!({
            "jti":jti,"htm":"POST","htu":format!("{PUBLIC_ORIGIN}{path}"),"iat":NOW_SECONDS - 1,
            "ath":stamps::sha256_url(token.as_bytes()),
            "intent":stamps::sha256_url(stamps::canonical(intent).as_bytes())
        }))?;
        Ok(vec![
            ("authorization", format!("DPoP {token}")),
            ("dpop", proof),
        ])
    }
}

fn create_intent(operation: &str, name: &str) -> Value {
    json!({"v":1,"operation":"CreateWorkspace","organization":ORGANIZATION,"target":ORGANIZATION,
        "operationId":operation,"params":{"name":name,"description":""}})
}
fn create_body(operation: &str, name: &str) -> Value {
    json!({"organizationId":ORGANIZATION,"operationId":operation,"name":name,"description":""})
}

async fn answer(
    socket: &Path,
    path: &str,
    headers: &[(&str, String)],
    body: &Value,
) -> ConformanceResult<(u16, Value)> {
    let (status, bytes) = link_post_with(socket, path, headers, body).await?;
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };
    Ok((status, body))
}

async fn counts(root: &Path) -> ConformanceResult<(i64, i64, i64)> {
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("fleet-{ORGANIZATION}.db")))
        .connect()
        .await?;
    let count = |table: &str| format!("SELECT count(*) FROM {table}");
    let mut values = Vec::new();
    for table in [
        "fleet_workspaces",
        "organization_change_records",
        "organization_used_requests",
    ] {
        values.push(
            query_scalar::<sqlx_turso::Turso, i64>(sqlx_core::sql_str::AssertSqlSafe(count(table)))
                .fetch_one(&mut store)
                .await?,
        );
    }
    store.close().await?;
    let [workspaces, records, markers] = values.as_slice() else {
        return Err("three counts expected".into());
    };
    Ok((*workspaces, *records, *markers))
}

#[expect(
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    reason = "One journey keeps every answer and the cold-read counts together while fixture errors propagate."
)]
#[tokio::test]
async fn organization_admin_creates_one_workspace_under_the_checked_permission()
-> ConformanceResult<()> {
    // Wrong builds caught: a caller-trusted guard; a member creating; a stale
    // factor changing state; a Bearer downgrade or wrong htu admitted; a retry
    // creating a second workspace or record; a reused proof admitted; a changed
    // body under the same operation ID applied; the fresh jti used as the
    // ledger identity.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let (server, socket) = start(root.path(), &access, &keys).await?;
    let provisioned = link_post(
        &socket,
        "/internal/organization-provisioning",
        &json!({"organizationId":ORGANIZATION,"organizationStatement":stamps::stamp(ORGANIZATION)?}),
    )
    .await?;
    server.shutdown().await?;
    seed_floors(root.path()).await?;
    let (server, socket) = start(root.path(), &access, &keys).await?;
    let owner = Person {
        id: OWNER,
        subject: OWNER_SUBJECT,
        revision: 4,
        permissions: vec![
            "organization:readAudit",
            "organization:view",
            "workspace:create",
        ],
        second_factor_at: NOW_SECONDS - 60,
        key: ProofKey::generate()?,
    };
    let member = Person {
        id: MEMBER,
        subject: MEMBER_SUBJECT,
        revision: 1,
        permissions: vec!["organization:view"],
        second_factor_at: NOW_SECONDS - 60,
        key: ProofKey::generate()?,
    };
    let stale_owner = Person {
        id: OWNER,
        subject: OWNER_SUBJECT,
        revision: 4,
        permissions: vec!["workspace:create"],
        second_factor_at: NOW_SECONDS - 901,
        key: ProofKey::generate()?,
    };
    let operation = "019f0000-0000-7000-8000-00000000d301";
    let other_operation = "019f0000-0000-7000-8000-00000000d302";
    let intent = create_intent(operation, "research");
    let body = create_body(operation, "research");

    let first_jti = fresh_request_id();
    let first_headers = owner.headers(CREATE, &intent, &first_jti)?;
    let created = answer(&socket, CREATE, &first_headers, &body).await?;
    let retried = answer(
        &socket,
        CREATE,
        &owner.headers(CREATE, &intent, &fresh_request_id())?,
        &body,
    )
    .await?;
    let reused = answer(&socket, CREATE, &first_headers, &body).await?;
    let changed_body = answer(
        &socket,
        CREATE,
        &owner.headers(
            CREATE,
            &create_intent(operation, "elsewhere"),
            &fresh_request_id(),
        )?,
        &create_body(operation, "elsewhere"),
    )
    .await?;
    let same_name = answer(
        &socket,
        CREATE,
        &owner.headers(
            CREATE,
            &create_intent(other_operation, "research"),
            &fresh_request_id(),
        )?,
        &create_body(other_operation, "research"),
    )
    .await?;
    let by_member = answer(
        &socket,
        CREATE,
        &member.headers(
            CREATE,
            &create_intent("019f0000-0000-7000-8000-00000000d303", "members"),
            &fresh_request_id(),
        )?,
        &create_body("019f0000-0000-7000-8000-00000000d303", "members"),
    )
    .await?;
    let stale = answer(
        &socket,
        CREATE,
        &stale_owner.headers(
            CREATE,
            &create_intent("019f0000-0000-7000-8000-00000000d304", "stale"),
            &fresh_request_id(),
        )?,
        &create_body("019f0000-0000-7000-8000-00000000d304", "stale"),
    )
    .await?;
    let mut bearer = owner.headers(CREATE, &intent, &fresh_request_id())?;
    if let Some(authorization) = bearer.first_mut() {
        authorization.1 = authorization.1.replacen("DPoP ", "Bearer ", 1);
    }
    let downgraded = answer(&socket, CREATE, &bearer, &body).await?;
    let no_proof = answer(
        &socket,
        CREATE,
        &owner.headers(CREATE, &intent, &fresh_request_id())?[..1],
        &body,
    )
    .await?;
    let wrong_htu = answer(
        &socket,
        CREATE,
        &owner.headers(LOOKUP, &intent, &fresh_request_id())?,
        &body,
    )
    .await?;
    let workspace = created
        .1
        .pointer("/workspace/workspaceId")
        .cloned()
        .unwrap_or(Value::Null);
    let lookup_intent = json!({"v":1,"operation":"WorkspaceLookup","organization":ORGANIZATION,
        "target":workspace,"operationId":null,"params":{}});
    let looked_up = answer(
        &socket,
        LOOKUP,
        &member.headers(LOOKUP, &lookup_intent, &fresh_request_id())?,
        &json!({"organizationId":ORGANIZATION,"workspaceId":workspace}),
    )
    .await?;
    let records_intent = json!({"v":1,"operation":"ReadChangeRecords","organization":ORGANIZATION,
        "target":ORGANIZATION,"operationId":null,"params":{"after":null,"limit":10}});
    let records = answer(
        &socket,
        RECORDS,
        &owner.headers(RECORDS, &records_intent, &fresh_request_id())?,
        &json!({"organizationId":ORGANIZATION,"limit":10}),
    )
    .await?;
    let public = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!("{}{CREATE}", server.base_url()))
        .json(&body)
        .send()
        .await?
        .status()
        .as_u16();
    server.shutdown().await?;
    let (workspaces, change_records, markers) = counts(root.path()).await?;

    assert_eq!(provisioned.0, 200);
    assert_eq!(created.0, 200, "{created:?}");
    assert_eq!(created.1.pointer("/result"), Some(&json!("applied")));
    assert_eq!(created.1.pointer("/alreadyExisted"), Some(&json!(false)));
    assert_eq!(
        workspace,
        json!(operation),
        "the workspace is named by its operation"
    );
    assert_eq!(
        retried.1.pointer("/workspace/workspaceId"),
        Some(&workspace)
    );
    assert_eq!(retried.1.pointer("/alreadyExisted"), Some(&json!(true)));
    assert_eq!(
        reused.1,
        json!({"result":"refused","reason":"requestReused"})
    );
    assert_eq!(
        changed_body.1,
        json!({"result":"refused","reason":"conflict"})
    );
    assert_eq!(
        same_name.1.pointer("/workspace/workspaceId"),
        Some(&workspace)
    );
    assert_eq!(same_name.1.pointer("/alreadyExisted"), Some(&json!(true)));
    assert_eq!(
        by_member.1,
        json!({"result":"refused","reason":"notPermitted"})
    );
    assert_eq!(
        stale.1,
        json!({"result":"refused","reason":"secondFactorRequired"})
    );
    assert_eq!((downgraded.0, no_proof.0, wrong_htu.0), (401, 401, 401));
    assert_eq!(looked_up.1.pointer("/result"), Some(&json!("found")));
    assert_eq!(
        looked_up.1.pointer("/workspace/name"),
        Some(&json!("research"))
    );
    let kinds = records
        .1
        .pointer("/records")
        .and_then(Value::as_array)
        .map(|records| {
            records
                .iter()
                .map(|record| {
                    (
                        record.pointer("/kind").cloned().unwrap_or(Value::Null),
                        record.pointer("/origin").cloned().unwrap_or(Value::Null),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert_eq!(
        kinds,
        vec![
            (
                json!("organizationProvisioned"),
                json!({"kind":"signInProvisioning"})
            ),
            (
                json!("workspaceCreated"),
                json!({"kind":"person","personId":OWNER,"permissionChecked":"workspace:create"})
            ),
        ]
    );
    assert_eq!(public, 404);
    // One workspace, setup + one creation record, and a marker for every
    // admitted request (refused ones reserve nothing): create, retry,
    // changed body, same name, lookup, records.
    assert_eq!((workspaces, change_records, markers), (1, 2, 6));
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}

const PUSH: &str = "/internal/organization-authorization-revisions";

fn organization_push(revision: i64, state: &str) -> Value {
    json!({"kind":"organization","organizationId":ORGANIZATION,"revision":revision,"state":state,
        "permissionPolicyDigest":POLICY})
}
fn person_push(revision: i64, state: &str, subject: &str) -> Value {
    json!({"kind":"person","organizationId":ORGANIZATION,"personId":OWNER,"subjectId":subject,
        "revision":revision,"state":state})
}

/// The person floors on disk; read only while the server is stopped.
async fn person_floors(root: &std::path::Path) -> ConformanceResult<Vec<(String, i64)>> {
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("fleet-{ORGANIZATION}.db")))
        .connect()
        .await?;
    let floors = sqlx_core::query_as::query_as(
        "SELECT person_id, minimum_revision FROM person_organization_authorization_floors",
    )
    .fetch_all(&mut store)
    .await?;
    store.close().await?;
    Ok(floors)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare each push answer and the retained floors while fixture errors propagate."
)]
#[tokio::test]
async fn organization_floor_push_acknowledges_only_what_commits() -> ConformanceResult<()> {
    // Wrong builds caught: a floor applied to an unstamped Fleet; a person
    // floor before its organization; an older push lowering a floor; a
    // conflicting payload accepted; an ACK for a rolled-back apply; a write that
    // commits behind a 503.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let (server, socket) = start(root.path(), &access, &keys).await?;
    let unstamped = link_post(&socket, PUSH, &organization_push(3, "active")).await?;
    let provisioned = link_post(
        &socket,
        "/internal/organization-provisioning",
        &json!({"organizationId":ORGANIZATION,"organizationStatement":stamps::stamp(ORGANIZATION)?}),
    )
    .await?;
    let early_person = link_post(&socket, PUSH, &person_push(4, "active", OWNER_SUBJECT)).await?;
    let organization = link_post(&socket, PUSH, &organization_push(3, "active")).await?;
    let older = link_post(&socket, PUSH, &organization_push(2, "stopped")).await?;
    let conflicting = link_post(&socket, PUSH, &organization_push(3, "stopped")).await?;
    reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!(
            "{}/admin/v1/conformance/runtime",
            server.base_url()
        ))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&RuntimeControlRequest {
            address: ObjectAddress::Fleet {
                organization: ORGANIZATION.to_owned().try_into()?,
            },
            command: RuntimeControl::FailNextOrganizationRecord,
        })
        .send()
        .await?
        .error_for_status()?;
    let faulted = link_post(&socket, PUSH, &person_push(4, "active", OWNER_SUBJECT)).await?;
    // Observe the store before any retry: an idempotent retry would hide a
    // write that committed behind the 503.
    server.shutdown().await?;
    let after_fault = person_floors(root.path()).await?;
    let (server, socket) = start(root.path(), &access, &keys).await?;
    let base = server.base_url().to_owned();
    let person = link_post(&socket, PUSH, &person_push(4, "active", OWNER_SUBJECT)).await?;
    let other_subject = link_post(
        &socket,
        PUSH,
        &person_push(5, "active", "019f0000-0000-7000-8000-00000000d2ff"),
    )
    .await?;
    let public = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!("{base}{PUSH}"))
        .json(&organization_push(9, "active"))
        .send()
        .await?
        .status()
        .as_u16();
    server.shutdown().await?;
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.path().join(format!("fleet-{ORGANIZATION}.db")))
        .connect()
        .await?;
    let floors: (i64, String, i64, String) = sqlx_core::query_as::query_as(
        "SELECT o.minimum_revision, o.state, p.minimum_revision, p.id FROM organization_authorization_floors o, person_organization_authorization_floors p",
    )
    .fetch_one(&mut store)
    .await?;
    store.close().await?;

    assert_eq!(
        unstamped.0, 503,
        "an unstamped Fleet keeps the push pending"
    );
    assert_eq!(provisioned.0, 200);
    assert_eq!(early_person.0, 412);
    assert_eq!(
        organization,
        (200, br#"{"acknowledgedRevision":3}"#.to_vec())
    );
    assert_eq!(older, (200, br#"{"acknowledgedRevision":3}"#.to_vec()));
    assert_eq!(conflicting.0, 409);
    assert_eq!(faulted.0, 503, "a rolled-back apply never acknowledges");
    assert_eq!(
        after_fault,
        Vec::<(String, i64)>::new(),
        "the faulted apply left no person floor behind"
    );
    assert_eq!(person, (200, br#"{"acknowledgedRevision":4}"#.to_vec()));
    assert_eq!(other_subject.0, 409);
    assert_eq!(public, 404);
    assert_eq!(floors, (3, "active".into(), 4, OWNER_SUBJECT.into()));
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}
