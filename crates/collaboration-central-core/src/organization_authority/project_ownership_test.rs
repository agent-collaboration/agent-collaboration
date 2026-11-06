use super::{ProjectOwnershipRefusal, require_project_ownership};
use crate::organization_authority::fleet_statement::tests::{
    FOREIGN_ORGANIZATION, ORGANIZATION, organization, verified_fixture,
};
use durable_steps::storage::TransactionScope;
use message_board::ProjectId;
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    executor::Executor,
    query_scalar::query_scalar,
};
use sqlx_turso::{Turso, TursoConnection};

const PROJECT: &str = "019f0000-0000-7000-8000-00000000ff01";
const FOREIGN_PROJECT: &str = "019f0000-0000-7000-8000-00000000ff02";
struct DirectoryScope<'connection>(&'connection mut TursoConnection);
impl TransactionScope for DirectoryScope<'_> {
    type Database = Turso;
    type Executor<'query>
        = &'query mut TursoConnection
    where
        Self: 'query;
    fn executor(&mut self) -> Self::Executor<'_> {
        self.0
    }
}

async fn directory() -> TursoConnection {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .connect()
        .await
        .unwrap();
    // This query/codec test isolates directory membership, not foreign-key admission.
    connection.execute("PRAGMA foreign_keys=OFF").await.unwrap();
    connection
        .execute(include_str!("../../core_migrations/0003_fleet_records.sql"))
        .await
        .unwrap();
    connection.execute("INSERT INTO fleet_project_directory(project_id,workspace_id,board_id,name,actor_kind,actor_human_id,actor_service_id,actor_endpoint_id,actor_session_id,actor_router_component,request_id,fleet_position,router_time,time_basis,origin_installation,origin_machine,import_digest,created_at,updated_at) VALUES('019f0000-0000-7000-8000-00000000ff01','019f0000-0000-7000-8000-00000000ff03','019f0000-0000-7000-8000-00000000ff04','literal-project','human','fixture-owner',NULL,NULL,NULL,NULL,NULL,1,NULL,'legacy_unknown','019f0000-0000-7000-8000-00000000ff05','fixture','abababababababababababababababababababababababababababababababab',1800000000000,1800000000000)").await.unwrap();
    connection
}

async fn changes(connection: &mut TursoConnection) -> i64 {
    query_scalar::<Turso, i64>("SELECT total_changes()")
        .fetch_one(connection)
        .await
        .unwrap()
}

#[tokio::test(start_paused = true)]
async fn fleet_ownership_accepts_its_directory_project_for_the_same_token_organization() {
    let mut connection = directory().await;
    let stamp = verified_fixture().await;
    let before = changes(&mut connection).await;
    let project = ProjectId::try_from(PROJECT.to_owned()).unwrap();

    let result = require_project_ownership(
        &mut DirectoryScope(&mut connection),
        &organization(ORGANIZATION),
        &stamp,
        &project,
    )
    .await;

    assert_eq!(result, Ok(()));
    assert_eq!(changes(&mut connection).await, before);
    connection.close().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn fleet_ownership_refuses_a_foreign_project_even_with_a_matching_token() {
    let mut connection = directory().await;
    let stamp = verified_fixture().await;
    let before = changes(&mut connection).await;
    let foreign = ProjectId::try_from(FOREIGN_PROJECT.to_owned()).unwrap();

    let result = require_project_ownership(
        &mut DirectoryScope(&mut connection),
        &organization(ORGANIZATION),
        &stamp,
        &foreign,
    )
    .await;

    assert_eq!(result, Err(ProjectOwnershipRefusal::ProjectUnavailable));
    assert_eq!(changes(&mut connection).await, before);
    connection.close().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn fleet_ownership_refuses_a_foreign_token_before_reading_the_directory() {
    // No directory table: an accidental subject read returns DirectoryUnavailable.
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .connect()
        .await
        .unwrap();
    let stamp = verified_fixture().await;
    let before = changes(&mut connection).await;
    let project = ProjectId::try_from(FOREIGN_PROJECT.to_owned()).unwrap();

    let result = require_project_ownership(
        &mut DirectoryScope(&mut connection),
        &organization(FOREIGN_ORGANIZATION),
        &stamp,
        &project,
    )
    .await;

    assert_eq!(result, Err(ProjectOwnershipRefusal::OrganizationMismatch));
    assert_eq!(changes(&mut connection).await, before);
    connection.close().await.unwrap();
}
