//! Acknowledgement stores the full handled version and never regresses it.
use crate::{
    fleet_record_codec::{corrupt, decode_json, required},
    generated_registry_lease as queries,
    lease_projection::LeaseProjectionVersion,
    step_driver::{StepCallback, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, Uuid, storage::TransactionScope};
use fleet_sync_protocol::DatabaseId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) enum LeaseAckStep {
    Read {
        database: DatabaseId,
    },
    Remember {
        database: DatabaseId,
        version: LeaseProjectionVersion,
        row_id: Uuid,
    },
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for LeaseAckStep
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<LeaseProjectionVersion>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Infallible>> {
        Box::pin(async move {
            let database = match self {
                Self::Read { database } | Self::Remember { database, .. } => database,
            };
            let project = database.as_uuid().to_string();
            let previous = queries::read_registry_lease_ack(scope, project.clone())
                .await
                .map_err(StoreError::backend)?;
            let previous = match previous {
                Some(row) => {
                    let row_id = required(row.row_id, "registry_lease_ack.row_id")?;
                    let parsed = Uuid::parse_str(&row_id)
                        .map_err(|error| corrupt("registry_lease_ack.row_id", error))?;
                    if !runtime_primitives::uuid_v7(parsed) || parsed.to_string() != row_id {
                        return Err(corrupt(
                            "registry_lease_ack.row_id",
                            "expected canonical UUIDv7",
                        ));
                    }
                    if required(row.project_id, "registry_lease_ack.project_id")? != project {
                        return Err(corrupt("registry_lease_ack.project_id", "identity differs"));
                    }
                    Some(decode_json::<LeaseProjectionVersion>(
                        &required(row.version, "registry_lease_ack.version")?,
                        "registry_lease_ack.version",
                    )?)
                }
                None => None,
            };
            if let Self::Remember {
                version, row_id, ..
            } = self
            {
                if !runtime_primitives::uuid_v7(row_id) {
                    return Err(corrupt(
                        "registry_lease_ack.row_id",
                        "runtime did not provide UUIDv7",
                    ));
                }
                if previous.is_none_or(|stored| version > stored) {
                    queries::store_registry_lease_ack(
                        scope,
                        row_id.to_string(),
                        project,
                        serde_json::to_string(&version).map_err(StoreError::backend)?,
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    return Ok((Some(version), Vec::new()));
                }
            }
            Ok((previous, Vec::new()))
        })
    }
}

pub(crate) struct ReadRegistrySnapshotIds;
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadRegistrySnapshotIds
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Vec<DatabaseId>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Infallible>> {
        Box::pin(async move {
            Ok((
                fleet_sync_lease::registry_snapshot_ids(scope)
                    .await
                    .map_err(StoreError::backend)?,
                Vec::new(),
            ))
        })
    }
}
