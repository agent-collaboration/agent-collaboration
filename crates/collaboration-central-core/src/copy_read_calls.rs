//! Shared snapshot reads cross Session → Copy only after the session commits.

use crate::{
    CopyKind, ObjectHost, ObjectRuntime, PlatformIo,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, CallTarget, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{
    CopyStatusAnswer, CopyUnavailableReason, DatabaseId, ReceiverId, ReplicationAnswer,
    ReplicationPosition, SnapshotCursor,
};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CopySessionRead {
    pub copy: ReceiverId,
    pub database: DatabaseId,
}
impl CallTarget for CopySessionRead {
    const KIND: &'static str = "central.copy-session-read";
    type Request = CopyReadRequest;
    type Answer = ReplicationAnswer;
}

/// A transaction that can only inspect the copy, never publish or apply it.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum CopyReadRequest {
    Status,
    Manifest { position: ReplicationPosition },
    Page { cursor: Option<SnapshotCursor> },
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<CopyKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> f64:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Vec<u8>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn read_copy(&self, request: CopyReadRequest) -> ReplicationAnswer {
        let ObjectWork::Copy { host, database, .. } = &self.work else {
            return fleet_sync_receiver::classify_copy_store_error(StoreError::Aborted {
                reason: "copy read reached a different face".into(),
            })
            .answer();
        };
        match StepDriver::new(host.clone())
            .run(ReadCopy {
                database: *database,
                request,
            })
            .await
        {
            Ok((ReplicationAnswer::CopyStatus(CopyStatusAnswer::NotInitialized), _)) => {
                ReplicationAnswer::CopyUnavailable {
                    reason: CopyUnavailableReason::Uninitialized,
                }
            }
            Ok((answer, _)) => answer,
            Err(error) => fleet_sync_receiver::classify_copy_store_error(error).answer(),
        }
    }
}

struct ReadCopy {
    database: DatabaseId,
    request: CopyReadRequest,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadCopy
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> f64: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Vec<u8>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = ReplicationAnswer;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let status = fleet_sync_receiver::copy_status(scope)
                .await
                .map_err(fleet_sync_receiver::receiver_rollback)?;
            if let CopyStatusAnswer::Status(status) = &status
                && status.database != self.database
            {
                return Err(StoreError::Corrupt {
                    field: "copy.database".into(),
                    message: "stored copy differs from its object".into(),
                });
            }
            let answer = match self.request {
                CopyReadRequest::Status => ReplicationAnswer::CopyStatus(status),
                CopyReadRequest::Manifest { position } => ReplicationAnswer::SnapshotManifest(
                    fleet_sync_receiver::snapshot_manifest(scope, position)
                        .await
                        .map_err(fleet_sync_receiver::receiver_rollback)?,
                ),
                CopyReadRequest::Page { cursor } => ReplicationAnswer::SnapshotPage(
                    fleet_sync_receiver::snapshot_page(scope, cursor)
                        .await
                        .map_err(fleet_sync_receiver::receiver_rollback)?,
                ),
            };
            Ok((answer, Vec::new()))
        })
    }
}
