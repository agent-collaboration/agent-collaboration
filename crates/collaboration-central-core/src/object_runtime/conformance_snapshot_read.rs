//! S8's durable read through the object's runtime face (seam contract v3.6).
//!
//! Lives under `object_runtime` because it needs the runtime's work; the
//! snapshot's types and its pure composition stay in `conformance_snapshot`.

use crate::conformance_snapshot::{DurableSnapshot, SnapshotRefusal, read_durable};
use crate::{ObjectKind, ObjectPlatform, ObjectRuntime, object_runtime::ObjectWork};
use durable_steps::{AtomicStore, ReborrowScope, StoreError, storage::TransactionScope};
use runtime_primitives::Clock;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

impl<Face: ObjectKind, Host: ObjectPlatform> ObjectRuntime<Face, Host>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    /// The durable half of S8, in one read-only transaction. A Copy has only
    /// its attempts; the Fleet fields stay at v3.3's explicit empties.
    pub async fn conformance_snapshot_durable(&self) -> Result<DurableSnapshot, SnapshotRefusal> {
        let (store, fleet, now) = match &self.work {
            ObjectWork::Fleet { host, io, .. } => (host.store().clone(), true, io.now()),
            ObjectWork::Copy { host, io, .. } => (host.store().clone(), false, io.now()),
        };
        let read = store
            .transaction(move |scope| {
                Box::pin(async move { Ok(read_durable(scope, fleet, now).await) })
            })
            .await;
        read.map_err(|error: StoreError| SnapshotRefusal::Storage(error.to_string()))?
    }
}
