//! Library-owned copy mutations release change notifications only after commit.

use crate::{
    CopyKind, ObjectHost, ObjectRuntime, PlatformIo,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, CallFailure, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{
    CreateCopy, CreateCopyAnswer, FenceRequest, FenceResult, RetireCopy, RetireCopyAnswer,
};
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) enum CopyMutation {
    Create(CreateCopy),
    Fence(FenceRequest),
    Retire(RetireCopy),
}

pub(crate) enum CopyMutationAnswer {
    Created(CreateCopyAnswer),
    Fenced(FenceResult),
    Retired(RetireCopyAnswer),
}

struct CopyChanged;
struct MutateCopy {
    mutation: CopyMutation,
    now: SyncTime,
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
    for<'value> f64: Type<Host::Database> + Encode<'value, Host::Database>,
    for<'value> Vec<u8>: Type<Host::Database> + Encode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn mutate_copy(
        &self,
        mutation: CopyMutation,
    ) -> Result<CopyMutationAnswer, CallFailure> {
        let ObjectWork::Copy {
            host, io, changes, ..
        } = &self.work
        else {
            return Err(CallFailure::AnswerLost);
        };
        let (answer, committed) = StepDriver::new(host.clone())
            .run(MutateCopy {
                mutation,
                now: io.now(),
            })
            .await
            .map_err(|error| {
                fleet_sync_receiver::classify_copy_store_error(error).call_failure()
            })?;
        for CopyChanged in committed.into_effects() {
            changes.signal_committed();
        }
        Ok(answer)
    }
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for MutateCopy
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> f64: Type<Driver> + Encode<'value, Driver>,
    for<'value> Vec<u8>: Type<Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = CopyMutationAnswer;
    type Effect = CopyChanged;

    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let answer = match self.mutation {
                CopyMutation::Create(request) => CopyMutationAnswer::Created(
                    fleet_sync_receiver::create_copy(scope, &request)
                        .await
                        .map_err(fleet_sync_receiver::receiver_rollback)?,
                ),
                CopyMutation::Fence(request) => CopyMutationAnswer::Fenced(
                    fleet_sync_receiver::fence(scope, &request)
                        .await
                        .map_err(fleet_sync_receiver::receiver_rollback)?,
                ),
                // The native library service treats an ambiguous retire as a
                // lost answer. Do not fabricate a no-effect classification.
                CopyMutation::Retire(request) => CopyMutationAnswer::Retired(
                    fleet_sync_receiver::retire_copy(scope, &request, self.now)
                        .await
                        .map_err(StoreError::backend)?,
                ),
            };
            Ok((answer, vec![CopyChanged]))
        })
    }
}
