//! The one seam a case needs from its host, and its single implementation over
//! any `AtomicStore` and SQLx driver. Cases are generic over `CaseHost` only,
//! so the driver bounds live here once.
use super::super::enrollment_port::MachineEnrollmentPort;
use super::super::enrollment_sql_store::SqlEnrollmentPort;
use super::case_values::{At, NOW_MS, ORGANIZATION, typed, uuid7};
use super::{CaseFailure, CasePhase, CaseResult};
use crate::machine_enrollment_record::{READMISSION_TASK_KIND, REVOKE_TASK_KIND};
use durable_steps::{
    AtomicStore, JournalScope, OperationContext, OperationFailure, RequestKey, StoreError, Task,
    storage::TransactionScope,
};
use fleet_identity_protocol::OrganizationId;
use runtime_primitives::SyncTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, executor::Executor, row::Row, types::Type,
};
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::Arc;

/// One of today's machines, as 0001/0002 code stores it.
#[derive(Clone, Debug)]
pub(crate) struct LegacyMachine {
    pub peer: String,
    pub service: String,
    pub label: String,
    pub last_seen: Option<String>,
    pub created_at: String,
}

/// Every machine-login row, keyed `<table>/<id>`, as the store holds it.
pub(crate) type Snapshot = BTreeMap<String, Value>;

pub(crate) trait CaseHost {
    type Port: MachineEnrollmentPort;
    fn port(&self) -> &Self::Port;
    /// Raw SQL, bypassing every Rust check: for seeding and engine probes.
    async fn execute(&self, statement: String) -> Result<(), StoreError>;
    /// The text column `value` of each row.
    async fn select(&self, statement: String) -> Result<Vec<String>, StoreError>;
    /// Today's Fleet (central-fleet, then registry v1/v2) with these machines.
    async fn initialize_pre_b3(&self, machines: Vec<LegacyMachine>) -> Result<(), StoreError>;
    /// B3's switch: the staged v3/v4. Returns how many applied.
    async fn initialize_b3(&self) -> Result<usize, StoreError>;
    /// Admits, never runs, one of today's revocation or readmission tasks.
    async fn admit_legacy_task(&self, revoke: bool) -> Result<(), StoreError>;
    /// Every change record, as the store's own reader returns them.
    async fn records(
        &self,
    ) -> Result<Vec<crate::organization_authority::OrganizationChangeRecord>, StoreError>;
}

/// The store's full machine-login readback: installations, enrollments,
/// allowances and change records, every column.
pub(crate) async fn snapshot<Host: CaseHost>(host: &Host) -> CaseResult<Snapshot> {
    let mut rows = Snapshot::new();
    for (table, columns) in [
        (
            "machine_installations",
            &[
                "id",
                "former_service_id",
                "label",
                "enrollment_generation",
                "last_seen_at",
                "created_at",
                "updated_at",
            ][..],
        ),
        (
            "machine_enrollments",
            &[
                "id",
                "installation_id",
                "request_id",
                "state",
                "end_reason",
                "approved_by_person_id",
                "permission_checked",
                "base_generation",
                "code_expires_at",
                "machine_client_id",
                "last_grant_at",
                "ended_at",
                "created_at",
                "updated_at",
            ][..],
        ),
        (
            "machine_allowances",
            &[
                "id",
                "installation_id",
                "workspace_id",
                "state",
                "created_at",
                "updated_at",
            ][..],
        ),
        (
            "organization_change_records",
            &[
                "id",
                "request_id",
                "kind",
                "origin",
                "actor_person_id",
                "permission_checked",
                "target_id",
                "created_at",
                "updated_at",
            ][..],
        ),
    ] {
        let object = columns
            .iter()
            .map(|column| format!("'{column}', {column}"))
            .collect::<Vec<_>>()
            .join(", ");
        for row in host
            .select(format!(
                "SELECT json_array(id, json_object({object})) AS value FROM {table}"
            ))
            .await?
        {
            let (id, value): (String, Value) = serde_json::from_str(&row)
                .map_err(|error| CaseFailure(format!("{table} readback: {error}")))?;
            rows.insert(format!("{table}/{id}"), value);
        }
    }
    Ok(rows)
}

pub(crate) struct SqlCaseHost<Store: AtomicStore, Driver: Database> {
    store: Arc<Store>,
    port: SqlEnrollmentPort<Store, Driver>,
    driver: PhantomData<fn() -> Driver>,
}

/// A stand-in for today's journal task kinds: admitted, never run.
struct LegacyTask<const REVOKE: bool, Store>(PhantomData<fn() -> Store>);
#[derive(Serialize, Deserialize)]
struct NoInput;
impl<const REVOKE: bool, Store: AtomicStore> Task for LegacyTask<REVOKE, Store> {
    const KIND: &'static str = if REVOKE {
        REVOKE_TASK_KIND
    } else {
        READMISSION_TASK_KIND
    };
    const VERSION: u32 = 2;
    type Store = Store;
    type Input = NoInput;
    type Output = ();
    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        _: &mut Context,
        _: &NoInput,
    ) -> Result<(), OperationFailure> {
        Ok(())
    }
}

/// Runs one case on this store, which must be fresh for every case except
/// `persistence`'s read, which runs on the store its write left behind.
/// Returns `Ok(())` or why the case failed.
///
/// # Errors
/// The failed step and what it observed.
pub async fn run_sql_case<Store, Driver>(
    name: &str,
    phase: CasePhase,
    store: Arc<Store>,
) -> Result<(), String>
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'blob> Vec<u8>: Type<Driver> + Encode<'blob, Driver> + Decode<'blob, Driver>,
    for<'scalar> i64: Type<Driver> + Encode<'scalar, Driver> + Decode<'scalar, Driver>,
    for<'blob> Option<Vec<u8>>: Type<Driver> + Decode<'blob, Driver>,
    for<'scalar> Option<i64>: Type<Driver> + Decode<'scalar, Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    let organization: OrganizationId = typed(uuid7(ORGANIZATION)).map_err(|failure| failure.0)?;
    let host = SqlCaseHost::<Store, Driver> {
        port: SqlEnrollmentPort::new(store.clone(), organization),
        store,
        driver: PhantomData,
    };
    super::dispatch(name, phase, &host)
        .await
        .map_err(|failure| format!("{name}: {}", failure.0))
}

impl<Store, Driver> CaseHost for SqlCaseHost<Store, Driver>
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'blob> Vec<u8>: Type<Driver> + Encode<'blob, Driver> + Decode<'blob, Driver>,
    for<'scalar> i64: Type<Driver> + Encode<'scalar, Driver> + Decode<'scalar, Driver>,
    for<'blob> Option<Vec<u8>>: Type<Driver> + Decode<'blob, Driver>,
    for<'scalar> Option<i64>: Type<Driver> + Decode<'scalar, Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Port = SqlEnrollmentPort<Store, Driver>;

    fn port(&self) -> &Self::Port {
        &self.port
    }

    async fn execute(&self, statement: String) -> Result<(), StoreError> {
        self.store
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
            .await
    }

    async fn select(&self, statement: String) -> Result<Vec<String>, StoreError> {
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let rows = sqlx_core::query::query::<Driver>(
                        sqlx_core::sql_str::AssertSqlSafe(statement),
                    )
                    .fetch_all(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                    rows.iter()
                        .map(|row| row.try_get::<String, _>("value"))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(StoreError::backend)
                })
            })
            .await
    }

    async fn initialize_pre_b3(&self, machines: Vec<LegacyMachine>) -> Result<(), StoreError> {
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    crate::object_bootstrap::initialize_pre_b3_fleet(
                        scope,
                        SyncTime::from_unix_millis(NOW_MS),
                        &At(NOW_MS),
                    )
                    .await?;
                    for machine in machines {
                        crate::generated_enrollment::insert_enrollment(
                            scope,
                            machine.peer.clone(),
                            machine.service,
                            machine.label,
                            format!("legacy-client-{}", machine.peer),
                            format!("legacy-hash-{}", machine.peer),
                            "2033-05-18T03:33:20+00:00".to_owned(),
                            machine.created_at.clone(),
                            machine.created_at,
                        )
                        .await
                        .map_err(StoreError::backend)?;
                        if let Some(last_seen) = machine.last_seen {
                            scope
                                .executor()
                                .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
                                    "UPDATE machine_enrollments SET last_seen_at = '{last_seen}' \
                                     WHERE machine_id = '{}'",
                                    machine.peer
                                )))
                                .await
                                .map_err(StoreError::backend)?;
                        }
                    }
                    Ok(())
                })
            })
            .await
    }

    async fn initialize_b3(&self) -> Result<usize, StoreError> {
        self.store
            .transaction(|scope| {
                Box::pin(async move {
                    crate::object_bootstrap::initialize_staged_b3_registry(
                        scope,
                        SyncTime::from_unix_millis(NOW_MS),
                        &At(NOW_MS),
                    )
                    .await
                })
            })
            .await
    }

    async fn admit_legacy_task(&self, revoke: bool) -> Result<(), StoreError> {
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let key =
                        RequestKey::new(format!("legacy:{revoke}")).map_err(StoreError::backend)?;
                    let admitted = if revoke {
                        scope.admit::<LegacyTask<true, Store>>(&key, &NoInput).await
                    } else {
                        scope
                            .admit::<LegacyTask<false, Store>>(&key, &NoInput)
                            .await
                    };
                    admitted.map_err(|refusal| StoreError::Aborted {
                        reason: format!("{refusal:?}"),
                    })?;
                    Ok(())
                })
            })
            .await
    }

    async fn records(
        &self,
    ) -> Result<Vec<crate::organization_authority::OrganizationChangeRecord>, StoreError> {
        self.store
            .transaction(|scope| {
                Box::pin(async move {
                    crate::organization_authority::change_records::read_page(scope, None, 100).await
                })
            })
            .await
    }
}
