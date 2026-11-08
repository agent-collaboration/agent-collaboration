use super::read_organization_records;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use durable_steps::storage::TransactionScope;
use fleet_identity_protocol::OrganizationId;
use sha2::{Digest, Sha256};
use sqlx_core::{connection::ConnectOptions, executor::Executor};
use sqlx_turso::{Turso, TursoConnection};

const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000ab01";
const OTHER_ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000ab02";
const NOW: i64 = 1_800_000_000_000;
/// More than one page of the change-record pager, so a capped read shows.
const RECORDS: usize = 250;

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

fn uuid(suffix: usize) -> String {
    format!("019f0000-0000-7000-8000-{suffix:012x}")
}

/// A compact statement whose header and claims the observer reads as bytes;
/// the stored row is never verified by the read.
fn statement() -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"t","alg":"EdDSA","kid":"key-observed"}"#);
    let claims = URL_SAFE_NO_PAD.encode(br#"{"iat":1800000000,"exp":1800003600}"#);
    format!("{header}.{claims}.signature")
}

async fn execute(connection: &mut TursoConnection, sql: String) {
    sqlx_core::query::query::<Turso>(sqlx_core::sql_str::AssertSqlSafe(sql))
        .execute(connection)
        .await
        .unwrap();
}

async fn empty_store() -> TursoConnection {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .connect()
        .await
        .unwrap();
    for migration in [
        include_str!("../core_migrations/0001_fleet_decisions.sql"),
        include_str!("../core_migrations/0003_fleet_records.sql"),
        include_str!("../organization_migrations/0011_organization_identity.sql"),
        include_str!("../organization_migrations/0012_organization_change_records.sql"),
        include_str!("../organization_migrations/0013_organization_authorization_floors.sql"),
        include_str!(
            "../organization_migrations/0014_person_organization_authorization_floors.sql"
        ),
        include_str!("../organization_migrations/0015_organization_used_requests.sql"),
    ] {
        connection.execute(migration).await.unwrap();
    }
    connection
}

async fn stamped_store() -> TursoConnection {
    let mut connection = empty_store().await;
    execute(
        &mut connection,
        format!(
            "INSERT INTO organization_identity VALUES('{ORGANIZATION}','{}',{NOW},{NOW})",
            statement()
        ),
    )
    .await;
    for index in 0..RECORDS {
        execute(
            &mut connection,
            format!(
                "INSERT INTO organization_change_records VALUES('{}','request-{index}','workspaceCreated','person','person-a','workspace.create','{}',{NOW},{NOW})",
                uuid(0x1000 + index),
                uuid(0x9000 + index),
            ),
        )
        .await;
    }
    execute(
        &mut connection,
        format!("INSERT INTO organization_authorization_floors VALUES('{ORGANIZATION}',7,'active','policy-7',0,{NOW},{NOW})"),
    )
    .await;
    for (subject, organization, person) in [
        (1, ORGANIZATION, "person-a"),
        (2, ORGANIZATION, "person-b"),
        (3, OTHER_ORGANIZATION, "person-a"),
    ] {
        execute(
            &mut connection,
            format!(
                "INSERT INTO person_organization_authorization_floors VALUES('{}','{organization}','{person}',{subject},'active',{NOW},{NOW})",
                uuid(0x2000 + subject),
            ),
        )
        .await;
    }
    for (request, organization) in [
        (1, ORGANIZATION),
        (2, ORGANIZATION),
        (3, OTHER_ORGANIZATION),
    ] {
        execute(
            &mut connection,
            format!(
                "INSERT INTO organization_used_requests VALUES('{}','{organization}',{NOW},'operation-{request}','person-a','session-a','intent-{request}','thumbprint',{NOW},{NOW})",
                uuid(0x3000 + request),
            ),
        )
        .await;
    }
    execute(
        &mut connection,
        format!(
            "INSERT INTO fleet_workspaces(workspace_id,name,description,state,repository_count,actor_kind,request_id,fleet_position,time_basis,origin_installation,origin_machine,created_at,updated_at) VALUES('{}','alpha','','active',0,'human','request-0',1,'router','installation-a','machine-a',{NOW},{NOW})",
            uuid(0x9000)
        ),
    )
    .await;
    execute(
        &mut connection,
        format!(
            "INSERT INTO fleet_request_decisions(request_id,canonical_hash,workspace_id,actor_kind,fleet_position,time_basis,origin_installation,origin_machine,decision_kind,change_kind,reason_count,created_at,updated_at) VALUES('request-0','hash','{}','human',1,'router','installation-a','machine-a','applied','workspaceCreated',0,{NOW},{NOW})",
            uuid(0x9000)
        ),
    )
    .await;
    connection
}

/// Rows written on this connection so far; a read that writes moves it.
async fn writes(connection: &mut TursoConnection) -> u64 {
    crate::session_finalize_fault::changes(&mut FleetScope(connection))
        .await
        .unwrap()
}

fn organization(value: &str) -> OrganizationId {
    OrganizationId::try_from(value.to_owned()).unwrap()
}

#[tokio::test]
async fn the_observer_reads_every_row_the_organization_keeps_and_writes_nothing() {
    // Arrange
    let mut connection = stamped_store().await;
    let before = writes(&mut connection).await;

    // Act
    let observed = read_organization_records(
        &mut FleetScope(&mut connection),
        &organization(ORGANIZATION),
    )
    .await
    .unwrap();

    // Assert
    assert_eq!(writes(&mut connection).await, before, "the observer wrote");
    assert!(observed.complete);
    assert_eq!(observed.statement.as_deref(), Some(statement().as_str()));
    assert_eq!(
        observed.stamped_organization,
        Some(organization(ORGANIZATION))
    );
    let stamp = observed.stamp.unwrap();
    assert_eq!(
        stamp.digest,
        format!("{:x}", Sha256::digest(statement().as_bytes()))
    );
    assert_eq!(stamp.key_id.as_deref(), Some("key-observed"));
    assert_eq!(
        (stamp.issued_at, stamp.expires_at),
        (Some(1_800_000_000), Some(1_800_003_600))
    );
    let ids: Vec<String> = observed
        .records
        .iter()
        .map(|record| record.id.to_string())
        .collect();
    let expected: Vec<String> = (0..RECORDS).map(|index| uuid(0x1000 + index)).collect();
    assert_eq!(
        ids, expected,
        "every change record, in identity order, none capped"
    );
    let floor = observed.organization_floor.unwrap();
    assert_eq!((floor.revision, floor.state.as_str()), (7, "active"));
    let people: Vec<(&str, i64)> = observed
        .person_floors
        .iter()
        .map(|floor| (floor.person_id.as_str(), floor.revision))
        .collect();
    assert_eq!(
        people,
        [("person-a", 1), ("person-b", 2)],
        "only this organization's people"
    );
    let used: Vec<&str> = observed
        .used_requests
        .iter()
        .map(|used| used.intent_digest.as_str())
        .collect();
    assert_eq!(
        used,
        ["intent-1", "intent-2"],
        "only this organization's markers"
    );
    assert_eq!(observed.workspaces.len(), 1);
    assert_eq!(observed.workspaces[0].name, "alpha");
    assert_eq!(observed.decisions.len(), 1);
    assert_eq!(
        observed.decisions[0].change_kind.as_deref(),
        Some("workspaceCreated")
    );
}

#[tokio::test]
async fn the_observer_reads_an_unstamped_store_as_empty_and_writes_nothing() {
    // Arrange
    let mut connection = empty_store().await;
    let before = writes(&mut connection).await;

    // Act
    let observed = read_organization_records(
        &mut FleetScope(&mut connection),
        &organization(ORGANIZATION),
    )
    .await
    .unwrap();

    // Assert
    assert_eq!(writes(&mut connection).await, before, "the observer wrote");
    assert!(observed.complete);
    assert_eq!(
        (
            observed.statement,
            observed.stamped_organization,
            observed.stamp
        ),
        (None, None, None)
    );
    assert!(observed.records.is_empty() && observed.person_floors.is_empty());
    assert!(observed.used_requests.is_empty() && observed.workspaces.is_empty());
    assert!(observed.decisions.is_empty() && observed.organization_floor.is_none());
}
