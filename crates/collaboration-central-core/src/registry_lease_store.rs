//! Bind committed public library snapshots and central's own incarnation to one owner.
use crate::{
    ObjectHost, RegistryIncarnation,
    fleet_record_codec::{corrupt, required},
    generated_registry_lease as queries,
    lease_projection::LeaseProjectionVersion,
    registry_lease_ack::{LeaseAckStep, ReadRegistrySnapshotIds},
    registry_notice_admission::{RegistryNoticeStep, RegistryNoticeWork},
    registry_peer_lookup::service_for_peer,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, Uuid, storage::TransactionScope};
use fleet_sync_lease::RegistrySnapshot;
use fleet_sync_protocol::{DatabaseId, SyncTime};
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{
    convert::Infallible,
    sync::{Arc, Weak},
};

pub(crate) enum LeaseSnapshotRead {
    MissingDatabase,
    MissingHolder,
    Available {
        snapshot: RegistrySnapshot,
        holder: Option<ServiceId>,
    },
}

pub(crate) trait RegistryLeaseStore: Send + Sync {
    fn databases(&self) -> BoxFuture<'_, Result<Vec<DatabaseId>, StoreError>>;
    fn acknowledged(
        &self,
        database: DatabaseId,
    ) -> BoxFuture<'_, Result<Option<LeaseProjectionVersion>, StoreError>>;
    fn acknowledge(
        &self,
        database: DatabaseId,
        version: LeaseProjectionVersion,
        row_id: Uuid,
    ) -> BoxFuture<'_, Result<(), StoreError>>;
    fn snapshot(
        &self,
        database: DatabaseId,
    ) -> BoxFuture<'_, Result<LeaseSnapshotRead, StoreError>>;
    fn notices_pending(&self, database: DatabaseId) -> BoxFuture<'_, Result<bool, StoreError>>;
    fn admit_notices(
        &self,
        database: DatabaseId,
        now: SyncTime,
    ) -> BoxFuture<'_, Result<bool, StoreError>>;
    fn last_incarnation(&self) -> BoxFuture<'_, Result<u64, StoreError>>;
    fn remember_incarnation(
        &self,
        incarnation: RegistryIncarnation,
        row_id: Uuid,
    ) -> BoxFuture<'_, Result<(), StoreError>>;
}

struct BoundRegistryLeaseStore<Host> {
    host: Weak<Host>,
}
pub(crate) fn bind<Host: ObjectHost>(host: Weak<Host>) -> Arc<dyn RegistryLeaseStore>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    Arc::new(BoundRegistryLeaseStore { host })
}
impl<Host: ObjectHost> RegistryLeaseStore for BoundRegistryLeaseStore<Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    fn databases(&self) -> BoxFuture<'_, Result<Vec<DatabaseId>, StoreError>> {
        Box::pin(async move {
            let host = self.host.upgrade().ok_or_else(owner_unavailable)?;
            Ok(StepDriver::new(host).run(ReadRegistrySnapshotIds).await?.0)
        })
    }
    fn acknowledged(
        &self,
        database: DatabaseId,
    ) -> BoxFuture<'_, Result<Option<LeaseProjectionVersion>, StoreError>> {
        Box::pin(async move {
            let host = self.host.upgrade().ok_or_else(owner_unavailable)?;
            Ok(StepDriver::new(host)
                .run(LeaseAckStep::Read { database })
                .await?
                .0)
        })
    }
    fn acknowledge(
        &self,
        database: DatabaseId,
        version: LeaseProjectionVersion,
        row_id: Uuid,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        Box::pin(async move {
            let host = self.host.upgrade().ok_or_else(owner_unavailable)?;
            StepDriver::new(host)
                .run(LeaseAckStep::Remember {
                    database,
                    version,
                    row_id,
                })
                .await?;
            Ok(())
        })
    }
    fn snapshot(
        &self,
        database: DatabaseId,
    ) -> BoxFuture<'_, Result<LeaseSnapshotRead, StoreError>> {
        Box::pin(async move {
            let host = self.host.upgrade().ok_or_else(owner_unavailable)?;
            Ok(StepDriver::new(host)
                .run(ReadLeaseSnapshot { database })
                .await?
                .0)
        })
    }
    fn notices_pending(&self, database: DatabaseId) -> BoxFuture<'_, Result<bool, StoreError>> {
        Box::pin(async move {
            let host = self.host.upgrade().ok_or_else(owner_unavailable)?;
            Ok(matches!(
                StepDriver::new(host)
                    .run(RegistryNoticeStep::Pending { database })
                    .await?
                    .0,
                RegistryNoticeWork::Pending | RegistryNoticeWork::Unavailable
            ))
        })
    }
    fn admit_notices(
        &self,
        database: DatabaseId,
        now: SyncTime,
    ) -> BoxFuture<'_, Result<bool, StoreError>> {
        Box::pin(async move {
            let host = self.host.upgrade().ok_or_else(owner_unavailable)?;
            Ok(matches!(
                StepDriver::new(host)
                    .run(RegistryNoticeStep::Admit { database, now })
                    .await?
                    .0,
                RegistryNoticeWork::Admitted
            ))
        })
    }
    fn last_incarnation(&self) -> BoxFuture<'_, Result<u64, StoreError>> {
        Box::pin(async move {
            let host = self.host.upgrade().ok_or_else(owner_unavailable)?;
            Ok(StepDriver::new(host).run(IncarnationStep::Read).await?.0)
        })
    }
    fn remember_incarnation(
        &self,
        incarnation: RegistryIncarnation,
        row_id: Uuid,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        Box::pin(async move {
            let host = self.host.upgrade().ok_or_else(owner_unavailable)?;
            StepDriver::new(host)
                .run(IncarnationStep::Remember {
                    incarnation,
                    row_id,
                })
                .await?;
            Ok(())
        })
    }
}

struct ReadLeaseSnapshot {
    database: DatabaseId,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadLeaseSnapshot
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = LeaseSnapshotRead;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Infallible>> {
        Box::pin(async move {
            let Some(snapshot) = fleet_sync_lease::registry_snapshot(scope, self.database)
                .await
                .map_err(StoreError::backend)?
            else {
                return Ok((LeaseSnapshotRead::MissingDatabase, Vec::new()));
            };
            let holder = match &snapshot.grant {
                Some(grant) => match service_for_peer(scope, grant.holder).await? {
                    Some(holder) => Some(holder),
                    None => return Ok((LeaseSnapshotRead::MissingHolder, Vec::new())),
                },
                None => None,
            };
            Ok((
                LeaseSnapshotRead::Available { snapshot, holder },
                Vec::new(),
            ))
        })
    }
}

enum IncarnationStep {
    Read,
    Remember {
        incarnation: RegistryIncarnation,
        row_id: Uuid,
    },
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for IncarnationStep
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = u64;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<u64, Infallible>> {
        Box::pin(async move {
            let row = queries::read_registry_incarnation(scope)
                .await
                .map_err(StoreError::backend)?;
            let previous = match row {
                Some(row) => {
                    let row_id = required(row.row_id, "registry_incarnation.row_id")?;
                    let parsed = Uuid::parse_str(&row_id)
                        .map_err(|error| corrupt("registry_incarnation.row_id", error))?;
                    if !runtime_primitives::uuid_v7(parsed) || parsed.to_string() != row_id {
                        return Err(corrupt(
                            "registry_incarnation.row_id",
                            "expected canonical UUIDv7",
                        ));
                    }
                    let value = required(row.incarnation, "registry_incarnation.value")?;
                    let incarnation = RegistryIncarnation::new(
                        value
                            .parse::<u64>()
                            .map_err(|error| corrupt("registry_incarnation.value", error))?,
                    )
                    .map_err(|error| corrupt("registry_incarnation.value", error))?;
                    if incarnation.value().to_string() != value {
                        return Err(corrupt(
                            "registry_incarnation.value",
                            "noncanonical integer",
                        ));
                    }
                    incarnation.value()
                }
                None => 0,
            };
            let next = match self {
                Self::Read => previous,
                Self::Remember {
                    incarnation,
                    row_id,
                } => {
                    if !runtime_primitives::uuid_v7(row_id) {
                        return Err(corrupt(
                            "registry_incarnation.row_id",
                            "expected UUIDv7 from runtime",
                        ));
                    }
                    if incarnation.value() > previous {
                        queries::store_registry_incarnation(
                            scope,
                            row_id.to_string(),
                            incarnation.value().to_string(),
                        )
                        .await
                        .map_err(StoreError::backend)?;
                    }
                    previous.max(incarnation.value())
                }
            };
            Ok((next, Vec::new()))
        })
    }
}
fn owner_unavailable() -> StoreError {
    StoreError::Aborted {
        reason: "Registry owner is no longer available".into(),
    }
}
