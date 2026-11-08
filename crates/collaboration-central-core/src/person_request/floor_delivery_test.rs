use super::{FloorDelivery, deliver};
use crate::identity_contract::AuthorizationRevision;
use durable_steps::storage::TransactionScope;
use serde_json::json;
use sqlx_core::{connection::ConnectOptions, executor::Executor, query_as::query_as};
use sqlx_turso::{Turso, TursoConnection};

const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000f201";
const PERSON: &str = "019f0000-0000-7000-8000-00000000f202";
const SUBJECT: &str = "019f0000-0000-7000-8000-00000000f203";
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

async fn store() -> TursoConnection {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .connect()
        .await
        .unwrap();
    for migration in [
        include_str!("../../organization_migrations/0013_organization_authorization_floors.sql"),
        include_str!(
            "../../organization_migrations/0014_person_organization_authorization_floors.sql"
        ),
    ] {
        connection.execute(migration).await.unwrap();
    }
    connection
}

fn organization(revision: i64, state: &str) -> AuthorizationRevision {
    serde_json::from_value(json!({"kind":"organization","organizationId":ORGANIZATION,
        "revision":revision,"state":state,"permissionPolicyDigest":POLICY}))
    .unwrap()
}
fn person(revision: i64, state: &str, subject: &str) -> AuthorizationRevision {
    serde_json::from_value(
        json!({"kind":"person","organizationId":ORGANIZATION,"personId":PERSON,
        "subjectId":subject,"revision":revision,"state":state}),
    )
    .unwrap()
}

async fn push(connection: &mut TursoConnection, revision: AuthorizationRevision) -> FloorDelivery {
    deliver(&mut FleetScope(connection), &revision, NOW)
        .await
        .unwrap()
}
fn acknowledged(revision: i64) -> FloorDelivery {
    FloorDelivery::Acknowledged(revision.try_into().unwrap())
}

#[tokio::test(start_paused = true)]
async fn floor_delivery_raises_never_lowers_and_never_resumes_a_deleted_organization() {
    // Wrong builds caught: an older push lowering the floor; a replayed push
    // with another payload overwriting it; a resume after deletion.
    let mut connection = store().await;

    let answers = vec![
        push(&mut connection, organization(2, "active")).await,
        push(&mut connection, organization(2, "active")).await,
        push(&mut connection, organization(2, "stopped")).await,
        push(&mut connection, organization(1, "active")).await,
        push(&mut connection, organization(3, "stopped")).await,
        push(&mut connection, organization(5, "deleted")).await,
        push(&mut connection, organization(6, "active")).await,
    ];
    let stored: (i64, String, i64) = query_as(
        "SELECT minimum_revision, state, request_reject_through_ms FROM organization_authorization_floors",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();

    assert_eq!(
        answers,
        vec![
            acknowledged(2),
            acknowledged(2),
            FloorDelivery::Conflict,
            acknowledged(2),
            acknowledged(3),
            acknowledged(5),
            acknowledged(5),
        ]
    );
    assert_eq!(stored, (5, "deleted".into(), 0));
}

#[tokio::test(start_paused = true)]
async fn floor_delivery_needs_the_organization_floor_and_keeps_one_subject_per_person() {
    // Wrong builds caught: a person floor without its organization; a second
    // subject ID taking a person's retained row; an older push lowering it.
    let mut connection = store().await;
    let early = push(&mut connection, person(1, "active", SUBJECT)).await;
    push(&mut connection, organization(1, "active")).await;

    let answers = vec![
        push(&mut connection, person(1, "active", SUBJECT)).await,
        push(&mut connection, person(4, "inactive", SUBJECT)).await,
        push(&mut connection, person(2, "active", SUBJECT)).await,
        push(&mut connection, person(4, "active", SUBJECT)).await,
        push(
            &mut connection,
            person(9, "active", "019f0000-0000-7000-8000-00000000f2ff"),
        )
        .await,
    ];
    let stored: (String, i64, String) = query_as(
        "SELECT id, minimum_revision, state FROM person_organization_authorization_floors",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();

    assert_eq!(early, FloorDelivery::OrganizationFloorMissing);
    assert_eq!(
        answers,
        vec![
            acknowledged(1),
            acknowledged(4),
            acknowledged(4),
            FloorDelivery::Conflict,
            FloorDelivery::Conflict,
        ]
    );
    assert_eq!(stored, (SUBJECT.into(), 4, "inactive".into()));
}

#[test]
fn floor_delivery_refuses_out_of_range_revisions_and_unknown_states_at_the_wire() {
    for body in [
        json!({"kind":"organization","organizationId":ORGANIZATION,"revision":0,"state":"active","permissionPolicyDigest":POLICY}),
        json!({"kind":"organization","organizationId":ORGANIZATION,"revision":9_007_199_254_740_992_i64,"state":"active","permissionPolicyDigest":POLICY}),
        json!({"kind":"organization","organizationId":ORGANIZATION,"revision":1,"state":"paused","permissionPolicyDigest":POLICY}),
        json!({"kind":"person","organizationId":ORGANIZATION,"personId":PERSON,"subjectId":SUBJECT,"revision":1,"state":"removed"}),
        json!({"kind":"person","organizationId":ORGANIZATION,"personId":PERSON,"subjectId":SUBJECT,"revision":1,"state":"active","acknowledgedRevision":2}),
    ] {
        assert!(
            serde_json::from_value::<AuthorizationRevision>(body.clone()).is_err(),
            "{body}"
        );
    }
}
