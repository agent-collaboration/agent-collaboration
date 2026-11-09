//! A real organization Fleet store on the Tokio host (sqlx-turso), initialized
//! as B3's deploy would: the central-fleet corpus, today's central-registry
//! corpus, optional legacy machines written by 0001/0002 code, then the staged
//! v3/v4. Test-only; production bootstrap is unchanged.
use super::enrollment_sql_store::SqlEnrollmentPort;
use super::test_fixtures::{ORGANIZATION, uuid7};
use crate::fleet_storage_fixture::FixtureIo;
use crate::{CoreHost, generated_enrollment};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_identity_protocol::OrganizationId;
use fleet_sync_protocol::ReceiverId;
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore, TokioObject};
use sqlx_core::executor::Executor;
use std::sync::Arc;

pub(super) type TestError = Box<dyn std::error::Error + Send + Sync>;
pub(super) type FixtureObject = TokioObject<CoreHost<FixtureIo>>;
pub(super) type FixturePort = SqlEnrollmentPort<FixtureObject, sqlx_turso::Turso>;

/// One of today's machines, as 0001/0002 code stores it.
pub(super) struct LegacyMachine {
    pub peer: String,
    pub service: String,
    pub label: String,
    pub last_seen: Option<String>,
    pub created_at: String,
}

pub(super) struct SqlStoreFixture {
    pub object: Arc<FixtureObject>,
    objects: ObjectStore<CoreHost<FixtureIo>>,
    root: tempfile::TempDir,
}

async fn open_object(
    root: &std::path::Path,
) -> Result<(ObjectStore<CoreHost<FixtureIo>>, Arc<FixtureObject>), TestError> {
    let objects = ObjectStore::open(root).await?;
    let host = Arc::new(CoreHost::for_store(
        FixtureIo,
        ReceiverId::new(durable_steps::Uuid::now_v7())?,
    ));
    let object = objects
        .object(ObjectKey::new("fleet")?, Default::default(), host)
        .await?;
    Ok((objects, object))
}

impl SqlStoreFixture {
    /// A store as it stands before B3, with these legacy machines.
    pub async fn pre_b3(machines: Vec<LegacyMachine>) -> Result<Self, TestError> {
        let root = tempfile::tempdir()?;
        let (objects, object) = open_object(root.path()).await?;
        object
            .transaction(move |scope| {
                Box::pin(async move {
                    crate::object_bootstrap::initialize_pre_b3_fleet(
                        scope,
                        crate::fleet_storage_fixture::NOW,
                        &FixtureIo,
                    )
                    .await?;
                    for machine in machines {
                        legacy_insert(scope, machine).await?;
                    }
                    Ok(())
                })
            })
            .await?;
        Ok(Self {
            object,
            objects,
            root,
        })
    }

    /// B3's deploy: the staged v3/v4 on this store. Returns how many applied.
    pub async fn initialize_b3(&self) -> Result<usize, StoreError> {
        self.object
            .transaction(|scope| {
                Box::pin(async move {
                    crate::object_bootstrap::initialize_staged_b3_registry(
                        scope,
                        crate::fleet_storage_fixture::NOW,
                        &FixtureIo,
                    )
                    .await
                })
            })
            .await
    }

    /// A B3 store with no legacy machines and these workspaces.
    pub async fn b3(workspaces: &[u64]) -> Result<Self, TestError> {
        let fixture = Self::pre_b3(Vec::new()).await?;
        fixture.initialize_b3().await?;
        for workspace in workspaces {
            fixture.add_workspace(*workspace).await?;
        }
        Ok(fixture)
    }

    pub fn port(&self) -> FixturePort {
        SqlEnrollmentPort::new(
            self.object.clone(),
            OrganizationId::try_from(uuid7(ORGANIZATION))
                .expect("the fixture organization is a UUIDv7"),
        )
    }

    pub async fn add_workspace(&self, suffix: u64) -> Result<(), TestError> {
        self.execute(format!(
            "INSERT INTO fleet_workspaces(workspace_id, name, description, state, repository_count, \
             actor_kind, fleet_position, time_basis, origin_installation, origin_machine, \
             created_at, updated_at) VALUES('{}', 'workspace {suffix}', '', 'active', 0, 'human', \
             {suffix}, 'router', '{}', 'fixture', 0, 0)",
            uuid7(suffix),
            uuid7(0x0f)
        ))
        .await
    }

    /// Raw SQL, bypassing every Rust check: for seeding and constraint probes.
    pub async fn execute(&self, statement: String) -> Result<(), TestError> {
        self.object
            .transaction(move |scope| {
                Box::pin(async move {
                    scope
                        .executor()
                        .execute(sqlx_core::sql_str::AssertSqlSafe(statement))
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await?;
        Ok(())
    }

    /// Every machine-login row and change record, as text, in a fixed order:
    /// the store's full readback for before-and-after comparisons.
    pub async fn rows(&self) -> Result<Vec<String>, TestError> {
        let mut rows = Vec::new();
        for (table, columns) in [
            (
                "machine_installations",
                "id, former_service_id, label, enrollment_generation, last_seen_at, created_at, updated_at",
            ),
            (
                "machine_enrollments",
                "id, installation_id, request_id, state, end_reason, approved_by_person_id, \
                 permission_checked, base_generation, code_expires_at, machine_client_id, \
                 last_grant_at, ended_at, created_at, updated_at",
            ),
            (
                "machine_allowances",
                "id, installation_id, workspace_id, state, created_at, updated_at",
            ),
            (
                "organization_change_records",
                "id, request_id, kind, origin, actor_person_id, permission_checked, target_id, \
                 created_at, updated_at",
            ),
        ] {
            let read: Vec<(String,)> = self
                .object
                .transaction(move |scope| {
                    Box::pin(async move {
                        sqlx_core::query_as::query_as::<sqlx_turso::Turso, (String,)>(
                            sqlx_core::sql_str::AssertSqlSafe(format!(
                                "SELECT json_array({columns}) FROM {table} ORDER BY 1"
                            )),
                        )
                        .fetch_all(scope.executor())
                        .await
                        .map_err(StoreError::backend)
                    })
                })
                .await?;
            rows.extend(read.into_iter().map(|(row,)| format!("{table}: {row}")));
        }
        Ok(rows)
    }

    /// One text column per row, from any query: for schema and history checks.
    pub async fn select(&self, statement: String) -> Result<Vec<String>, TestError> {
        let rows: Vec<(String,)> = self
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    sqlx_core::query_as::query_as::<sqlx_turso::Turso, (String,)>(
                        sqlx_core::sql_str::AssertSqlSafe(statement),
                    )
                    .fetch_all(scope.executor())
                    .await
                    .map_err(StoreError::backend)
                })
            })
            .await?;
        Ok(rows.into_iter().map(|(row,)| row).collect())
    }

    /// A new owning process's handle over the same persistence.
    pub async fn reopen(self) -> Result<Self, TestError> {
        let Self {
            object,
            objects,
            root,
        } = self;
        objects.close().await?;
        drop(object);
        let (objects, object) = open_object(root.path()).await?;
        Ok(Self {
            object,
            objects,
            root,
        })
    }
}

async fn legacy_insert<S: TransactionScope<Database = sqlx_turso::Turso>>(
    scope: &mut S,
    machine: LegacyMachine,
) -> Result<(), StoreError> {
    generated_enrollment::insert_enrollment(
        scope,
        machine.peer.clone(),
        machine.service,
        machine.label,
        format!("legacy-client-{}", machine.peer),
        format!("legacy-hash-{}", machine.peer),
        "2033-05-18T03:33:20+00:00".into(),
        machine.created_at.clone(),
        machine.created_at,
    )
    .await
    .map_err(StoreError::backend)?;
    if let Some(last_seen) = machine.last_seen {
        scope
            .executor()
            .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
                "UPDATE machine_enrollments SET last_seen_at = '{last_seen}' WHERE machine_id = '{}'",
                machine.peer
            )))
            .await
            .map_err(StoreError::backend)?;
    }
    Ok(())
}
