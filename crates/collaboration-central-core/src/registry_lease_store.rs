//! Read the directory projection that the Fleet's lease hook committed atomically.
use crate::{
    ObjectPlatform,
    fleet_record_codec::corrupt,
    lease_projection::LeaseProjection,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use fleet_protocol::DatabaseId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{
    convert::Infallible,
    sync::{Arc, Weak},
};

pub(crate) trait RegistryLeaseStore: Send + Sync {
    fn projection(
        &self,
        database: DatabaseId,
    ) -> BoxFuture<'_, Result<LeaseProjection, StoreError>>;
}
struct BoundRegistryLeaseStore<Host> {
    host: Weak<Host>,
}
pub(crate) fn bind<Host: ObjectPlatform>(host: Weak<Host>) -> Arc<dyn RegistryLeaseStore>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    Arc::new(BoundRegistryLeaseStore { host })
}
impl<Host: ObjectPlatform> RegistryLeaseStore for BoundRegistryLeaseStore<Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    fn projection(
        &self,
        database: DatabaseId,
    ) -> BoxFuture<'_, Result<LeaseProjection, StoreError>> {
        Box::pin(async move {
            let host = self
                .host
                .upgrade()
                .ok_or_else(|| StoreError::backend("Fleet owner is unavailable"))?;
            Ok(StepDriver::new(host)
                .run(ReadLeaseProjection { database })
                .await?
                .0)
        })
    }
}
struct ReadLeaseProjection {
    database: DatabaseId,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadLeaseProjection
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = LeaseProjection;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let row = crate::generated_fleet_lease::read_directory_lease(
                scope,
                self.database.to_string(),
            )
            .await
            .map_err(StoreError::backend)?
            .ok_or_else(|| corrupt("lease.project", "directory entry is missing"))?;
            let projection = crate::fleet_lease_columns::decode(row)?
                .ok_or_else(|| corrupt("lease.projection", "missing committed projection"))?;
            if projection.database() != self.database {
                return Err(corrupt(
                    "lease.project",
                    "projection belongs to another project",
                ));
            }
            Ok((projection, Vec::new()))
        })
    }
}
