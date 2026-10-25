//! Test-only typed readback of the same decoder used by Fleet's wire reads.
use crate::{
    FleetKind, ObjectHost, ObjectRuntime, PlatformIo,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use message_board::{Record, RecordRef};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

#[derive(Debug, thiserror::Error)]
#[error("Fleet read of {record:?} failed: {error}")]
pub struct FleetRecordReadFailure {
    pub record: RecordRef,
    #[source]
    pub error: StoreError,
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String: Type<Host::Database> + Encode<'value, Host::Database>,
    for<'value> Option<String>: Type<Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub async fn conformance_record_read(
        &self,
        reference: RecordRef,
    ) -> Result<Option<Record>, FleetRecordReadFailure> {
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Err(FleetRecordReadFailure {
                record: reference,
                error: StoreError::Aborted {
                    reason: "record observer reached a different face".into(),
                },
            });
        };
        StepDriver::new(host.clone())
            .run(ReadRecord(reference.clone()))
            .await
            .map(|(record, _)| record)
            .map_err(|error| FleetRecordReadFailure {
                record: reference,
                error,
            })
    }
}

struct ReadRecord(RecordRef);
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadRecord
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<Record>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let record = crate::fleet_record_store::read(scope, &self.0)
                .await?
                .map(|decoded| decoded.record);
            Ok((record, Vec::new()))
        })
    }
}
