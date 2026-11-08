use super::{CopyFleetReference, CopyFleetReferenceRefusal, bind, resolve};
use durable_steps::storage::TransactionScope;
use fleet_identity_protocol::OrganizationId;
use fleet_sync_protocol::DatabaseId;
use sqlx_core::{connection::ConnectOptions, executor::Executor, query_scalar::query_scalar};
use sqlx_turso::{Turso, TursoConnection};

const FIRST: &str = "019f0000-0000-7000-8000-00000000ad01";
const SECOND: &str = "019f0000-0000-7000-8000-00000000ad02";
const PROJECT: &str = "019f0000-0000-7000-8000-00000000ad03";
const OTHER_PROJECT: &str = "019f0000-0000-7000-8000-00000000ad04";
const NOW: i64 = 1_800_000_000_000;

struct CopyScope<'connection>(&'connection mut TursoConnection);
impl TransactionScope for CopyScope<'_> {
    type Database = Turso;
    type Executor<'query>
        = &'query mut TursoConnection
    where
        Self: 'query;
    fn executor(&mut self) -> Self::Executor<'_> {
        self.0
    }
}

fn project(value: &str) -> DatabaseId {
    value.parse().unwrap()
}
fn organization(value: &str) -> OrganizationId {
    value.to_owned().try_into().unwrap()
}
async fn copy_store(migrated: bool) -> TursoConnection {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .connect()
        .await
        .unwrap();
    if migrated {
        connection
            .execute(include_str!(
                "../copy_migrations/0002_copy_fleet_reference.sql"
            ))
            .await
            .unwrap();
    }
    connection
}

#[tokio::test(start_paused = true)]
async fn copy_fleet_reference_is_missing_before_the_copy_schema_exists() {
    // Wrong build caught: an uninitialized Copy failing the read instead of
    // reporting no note, which would refuse its first creation call.
    let mut connection = copy_store(false).await;

    let read = resolve(&mut CopyScope(&mut connection), project(PROJECT)).await;

    assert_eq!(read.unwrap(), CopyFleetReference::Missing);
}

#[tokio::test(start_paused = true)]
async fn copy_fleet_reference_survives_in_the_copy_store_after_binding() {
    let mut connection = copy_store(true).await;
    let mut scope = CopyScope(&mut connection);

    bind(&mut scope, project(PROJECT), &organization(FIRST), NOW)
        .await
        .unwrap()
        .unwrap();
    let read = resolve(&mut scope, project(PROJECT)).await.unwrap();
    let other = resolve(&mut scope, project(OTHER_PROJECT)).await.unwrap();

    assert_eq!(read, CopyFleetReference::Bound(organization(FIRST)));
    assert_eq!(other, CopyFleetReference::Missing);
    let stored: (String, String, i64, i64) = sqlx_core::query_as::query_as(
        "SELECT project_id, fleet_address_key, created_at, updated_at FROM _copy_fleet_reference",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(stored, (PROJECT.into(), FIRST.into(), NOW, NOW));
}

#[tokio::test(start_paused = true)]
async fn copy_fleet_reference_repeats_are_safe_but_another_fleet_cannot_take_it() {
    // Wrong build caught: a second organization overwriting the note, or a
    // same-organization repeat failing on the primary key.
    let mut connection = copy_store(true).await;
    let mut scope = CopyScope(&mut connection);
    bind(&mut scope, project(PROJECT), &organization(FIRST), NOW)
        .await
        .unwrap()
        .unwrap();

    let repeat = bind(&mut scope, project(PROJECT), &organization(FIRST), NOW + 1)
        .await
        .unwrap();
    let foreign = bind(&mut scope, project(PROJECT), &organization(SECOND), NOW + 2)
        .await
        .unwrap();

    assert_eq!(repeat, Ok(()));
    assert_eq!(
        foreign,
        Err(CopyFleetReferenceRefusal::OrganizationMismatch)
    );
    let rows: i64 = query_scalar::<Turso, i64>(
        "SELECT count(*) FROM _copy_fleet_reference WHERE fleet_address_key = ?",
    )
    .bind(FIRST)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(rows, 1);
}
