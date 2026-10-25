//! ImportTask reserves identity; it never answers admin publication completion.

use crate::{
    CallerKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo, RegistryKind,
    caller_context::CallAuthority,
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, JournalScope, NotSentReason, OperationId,
    OperationState, StoreError, storage::TransactionScope,
};
use fleet_sync_lease::{ImportTask, lifecycle_request_key};
use fleet_sync_protocol::{Import, ImportAnswer};
use runtime_primitives::{Clock, SyncTime};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

#[derive(Serialize, Deserialize)]
pub(crate) struct AdminImportReservation;
impl CallTarget for AdminImportReservation {
    const KIND: &'static str = "central.admin-import-reservation";
    type Request = Import;
    type Answer = ReservationAnswer;
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ReservationAnswer {
    Pending,
    Resolved(ImportAnswer),
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<RegistryKind, Host, Io>
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
    pub(crate) async fn admin_import_reservation(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source_face != CallerKind::Edge
            || call.source != ObjectAddress::Registry
            || !matches!(call.authority, CallAuthority::Owner(_))
        {
            return Err(refused());
        }
        let _: AdminImportReservation =
            serde_json::from_value(call.target).map_err(|_| refused())?;
        let input: Import = serde_json::from_value(call.request).map_err(|_| refused())?;
        let ObjectWork::Registry {
            host,
            engine,
            shared_host,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        let (operation, _) = StepDriver::new(host.clone())
            .run(AdmitImport {
                input,
                now: shared_host.now(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        engine
            .drive_due()
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        let answer = match engine
            .status(&operation)
            .await
            .map_err(|_| CallFailure::AnswerLost)?
            .state
        {
            OperationState::Completed { result } => ReservationAnswer::Resolved(
                serde_json::from_slice(&result).map_err(|_| CallFailure::AnswerLost)?,
            ),
            OperationState::Running { .. } => ReservationAnswer::Pending,
            OperationState::Failed { .. } => return Err(CallFailure::AnswerLost),
        };
        serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
    }
}

struct AdmitImport {
    input: Import,
    now: SyncTime,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for AdmitImport
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = OperationId;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let key = lifecycle_request_key(self.input.attempt).map_err(StoreError::backend)?;
            let admitted = scope
                .admit::<ImportTask<Store, Driver>>(&key, &self.input)
                .await
                .map_err(StoreError::backend)?;
            scope.ensure_wake(self.now).await?;
            Ok((admitted.id(), Vec::new()))
        })
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
