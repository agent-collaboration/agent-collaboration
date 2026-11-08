use super::{AdmissionRefusal, admit};
use crate::person_request::{
    AdminOperation, CreateWorkspaceRequest, VerifiedPersonRequest, WorkspaceLookupRequest,
};
use durable_steps::{Uuid, storage::TransactionScope};
use sqlx_core::{connection::ConnectOptions, executor::Executor, query_scalar::query_scalar};
use sqlx_turso::{Turso, TursoConnection};

const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000c201";
const PERSON: &str = "019f0000-0000-7000-8000-00000000c202";
const SUBJECT: &str = "019f0000-0000-7000-8000-00000000c203";
const SESSION: &str = "019f0000-0000-7000-8000-00000000c204";
const POLICY: &str = "2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22";
const NOW: i64 = 1_900_000_000_000;

struct FleetScope<'connection>(&'connection mut TursoConnection);
impl TransactionScope for FleetScope<'_> {
    type Database = Turso;
    type Executor<'query>
        = &'query mut TursoConnection
    where
        Self: 'query;
    fn executor(&mut self) -> Self::Executor<'_> {
        self.0
    }
}

/// Floors are seeded here only: central never creates one from a token.
async fn store(organization_floor: Option<&str>, person_floor: Option<&str>) -> TursoConnection {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .connect()
        .await
        .unwrap();
    for migration in [
        include_str!("../../organization_migrations/0013_organization_authorization_floors.sql"),
        include_str!(
            "../../organization_migrations/0014_person_organization_authorization_floors.sql"
        ),
        include_str!("../../organization_migrations/0015_organization_used_requests.sql"),
    ] {
        connection.execute(migration).await.unwrap();
    }
    if let Some(values) = organization_floor {
        let insert = format!(
            "INSERT INTO organization_authorization_floors VALUES('{ORGANIZATION}',{values},{NOW},{NOW})"
        );
        sqlx_core::query::query::<Turso>(sqlx_core::sql_str::AssertSqlSafe(insert))
            .execute(&mut connection)
            .await
            .unwrap();
    }
    if let Some(values) = person_floor {
        let insert = format!(
            "INSERT INTO person_organization_authorization_floors VALUES({values},{NOW},{NOW})"
        );
        sqlx_core::query::query::<Turso>(sqlx_core::sql_str::AssertSqlSafe(insert))
            .execute(&mut connection)
            .await
            .unwrap();
    }
    connection
}

const ACTIVE_ORGANIZATION: &str =
    "3,'active','2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22',1899999990000";
fn active_person() -> String {
    format!("'{SUBJECT}','{ORGANIZATION}','{PERSON}',4,'active'")
}

/// A request time is a UUIDv7's millisecond prefix.
fn request_at(millis: i64) -> Uuid {
    Uuid::parse_str(&format!(
        "{:08x}-{:04x}-7000-8000-00000000c299",
        millis >> 16,
        millis & 0xffff
    ))
    .unwrap()
}

fn create() -> VerifiedPersonRequest {
    VerifiedPersonRequest {
        person_id: PERSON.into(),
        session_id: SESSION.into(),
        organization_id: ORGANIZATION.to_owned().try_into().unwrap(),
        authorization_subject_id: SUBJECT.into(),
        person_revision: 4,
        organization_revision: 3,
        permission_policy_digest: POLICY.into(),
        permissions: vec!["workspace:create".into(), "organization:view".into()],
        second_factor_at: Some(NOW / 1000 - 60),
        request_id: request_at(NOW - 1_000),
        request_time_ms: NOW - 1_000,
        proof_time_ms: NOW - 1_000,
        proof_key_thumbprint: "thumbprint".into(),
        intent_digest: "intent".into(),
        effective_expiry_ms: NOW + 60_000,
        operation: AdminOperation::CreateWorkspace(CreateWorkspaceRequest {
            organization_id: ORGANIZATION.to_owned().try_into().unwrap(),
            operation_id: "019f0000-0000-7000-8000-00000000c205"
                .to_owned()
                .try_into()
                .unwrap(),
            name: String::from("research").try_into().unwrap(),
            description: String::new(),
        }),
    }
}

async fn markers(connection: &mut TursoConnection) -> i64 {
    query_scalar::<Turso, i64>("SELECT count(*) FROM organization_used_requests")
        .fetch_one(connection)
        .await
        .unwrap()
}

async fn outcome(
    organization_floor: Option<&str>,
    person_floor: Option<String>,
    request: VerifiedPersonRequest,
    now: i64,
) -> (Result<(), AdmissionRefusal>, i64) {
    let mut connection = store(organization_floor, person_floor.as_deref()).await;
    let admitted = admit(&mut FleetScope(&mut connection), &request, now)
        .await
        .unwrap()
        .map(|_| ());
    (admitted, markers(&mut connection).await)
}

#[tokio::test(start_paused = true)]
async fn person_admission_reserves_the_fresh_request_once() {
    // Wrong build caught: a replayed request ID admitted twice.
    let mut connection = store(Some(ACTIVE_ORGANIZATION), Some(&active_person())).await;
    let request = create();

    let first = admit(&mut FleetScope(&mut connection), &request, NOW)
        .await
        .unwrap();
    let again = admit(&mut FleetScope(&mut connection), &request, NOW)
        .await
        .unwrap();

    assert!(first.is_ok());
    assert_eq!(again.unwrap_err(), AdmissionRefusal::RequestReused);
    assert_eq!(markers(&mut connection).await, 1);
}

#[tokio::test(start_paused = true)]
async fn person_admission_refuses_without_the_pushed_floors_and_reserves_nothing() {
    // Wrong build caught: a token creating or skipping a missing floor.
    assert_eq!(
        outcome(None, Some(active_person()), create(), NOW).await,
        (Err(AdmissionRefusal::AuthorizationSyncRequired), 0)
    );
    assert_eq!(
        outcome(Some(ACTIVE_ORGANIZATION), None, create(), NOW).await,
        (Err(AdmissionRefusal::AuthorizationSyncRequired), 0)
    );
}

#[tokio::test(start_paused = true)]
async fn person_admission_compares_token_revisions_and_states_with_the_floors() {
    // Wrong builds caught: an old revision accepted; a newer one treated as
    // removed; a stopped or deleted organization, an inactive person, a
    // re-created subject or another policy admitted.
    let mut older_org = create();
    older_org.organization_revision = 2;
    let mut newer_org = create();
    newer_org.organization_revision = 4;
    let mut older_person = create();
    older_person.person_revision = 3;
    let mut newer_person = create();
    newer_person.person_revision = 5;
    let mut other_policy = create();
    other_policy.permission_policy_digest = "ab".repeat(32);
    let mut other_subject = create();
    other_subject.authorization_subject_id = "019f0000-0000-7000-8000-00000000c2ff".into();
    let stopped = ACTIVE_ORGANIZATION.replace("'active'", "'stopped'");
    let deleted = ACTIVE_ORGANIZATION.replace("'active'", "'deleted'");
    let inactive = active_person().replace("'active'", "'inactive'");
    for (organization, person, request, expected) in [
        (
            ACTIVE_ORGANIZATION,
            active_person(),
            older_org,
            AdmissionRefusal::RefreshRequired,
        ),
        (
            ACTIVE_ORGANIZATION,
            active_person(),
            newer_org,
            AdmissionRefusal::AuthorizationSyncRequired,
        ),
        (
            ACTIVE_ORGANIZATION,
            active_person(),
            older_person,
            AdmissionRefusal::RefreshRequired,
        ),
        (
            ACTIVE_ORGANIZATION,
            active_person(),
            newer_person,
            AdmissionRefusal::AuthorizationSyncRequired,
        ),
        (
            ACTIVE_ORGANIZATION,
            active_person(),
            other_policy,
            AdmissionRefusal::RefreshRequired,
        ),
        (
            ACTIVE_ORGANIZATION,
            active_person(),
            other_subject,
            AdmissionRefusal::NotPermitted,
        ),
        (
            stopped.as_str(),
            active_person(),
            create(),
            AdmissionRefusal::NotPermitted,
        ),
        (
            deleted.as_str(),
            active_person(),
            create(),
            AdmissionRefusal::NotPermitted,
        ),
        (
            ACTIVE_ORGANIZATION,
            inactive,
            create(),
            AdmissionRefusal::NotPermitted,
        ),
    ] {
        assert_eq!(
            outcome(Some(organization), Some(person), request, NOW).await,
            (Err(expected), 0)
        );
    }
}

#[tokio::test(start_paused = true)]
async fn person_admission_requires_the_route_permission_and_a_fresh_factor_for_changes() {
    // Wrong builds caught: a member without the permission admitted; a stale
    // factor allowed to change state; a read refused for its factor.
    let mut member = create();
    member.permissions = vec!["organization:view".into()];
    let mut stale = create();
    stale.second_factor_at = Some(NOW / 1000 - 901);
    let mut absent = create();
    absent.second_factor_at = None;
    let mut read = create();
    read.second_factor_at = None;
    read.operation = AdminOperation::WorkspaceLookup(WorkspaceLookupRequest {
        organization_id: ORGANIZATION.to_owned().try_into().unwrap(),
        workspace_id: "019f0000-0000-7000-8000-00000000c206"
            .to_owned()
            .try_into()
            .unwrap(),
    });
    for (request, expected) in [
        (member, Err(AdmissionRefusal::NotPermitted)),
        (stale, Err(AdmissionRefusal::SecondFactorRequired)),
        (absent, Err(AdmissionRefusal::SecondFactorRequired)),
        (read, Ok(())),
    ] {
        let markers_expected = i64::from(expected.is_ok());
        assert_eq!(
            outcome(
                Some(ACTIVE_ORGANIZATION),
                Some(active_person()),
                request,
                NOW
            )
            .await,
            (expected, markers_expected)
        );
    }
}

#[tokio::test(start_paused = true)]
async fn person_admission_rechecks_every_time_bound_on_the_applying_clock() {
    // Wrong builds caught: expiry or the request window checked only at
    // ingress; equality treated as valid; a request at the watermark admitted.
    let request = create();
    let mut at_watermark = create();
    at_watermark.request_id = request_at(1_899_999_990_000);
    at_watermark.request_time_ms = 1_899_999_990_000;
    for (request, now, expected) in [
        (request.clone(), NOW + 60_000, AdmissionRefusal::Expired),
        (request.clone(), NOW + 64_000, AdmissionRefusal::Expired),
        (request.clone(), NOW - 6_001, AdmissionRefusal::RequestTime),
        (at_watermark, NOW, AdmissionRefusal::RequestTime),
    ] {
        assert_eq!(
            outcome(
                Some(ACTIVE_ORGANIZATION),
                Some(active_person()),
                request,
                now
            )
            .await,
            (Err(expected), 0)
        );
    }
    let mut late = create();
    late.effective_expiry_ms = NOW + 120_000;
    assert_eq!(
        outcome(
            Some(ACTIVE_ORGANIZATION),
            Some(active_person()),
            late,
            NOW + 64_000
        )
        .await,
        (Err(AdmissionRefusal::RequestTime), 0),
        "a request older than 65 s at the applying clock is refused"
    );
}

#[tokio::test(start_paused = true)]
async fn person_admission_refuses_corrupt_floors_instead_of_reading_them() {
    // Wrong builds caught: an unknown state or out-of-range revision coerced.
    for organization in [
        ACTIVE_ORGANIZATION.replace("'active'", "'paused'"),
        ACTIVE_ORGANIZATION.replacen('3', "0", 1),
    ] {
        let mut connection = store(Some(&organization), Some(&active_person())).await;
        assert!(
            admit(&mut FleetScope(&mut connection), &create(), NOW)
                .await
                .is_err()
        );
        assert_eq!(markers(&mut connection).await, 0);
    }
    let mut connection = store(
        Some(ACTIVE_ORGANIZATION),
        Some(&active_person().replace("'active'", "'gone'")),
    )
    .await;
    assert!(
        admit(&mut FleetScope(&mut connection), &create(), NOW)
            .await
            .is_err()
    );
}
