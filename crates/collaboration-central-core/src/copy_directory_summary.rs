//! Copy metadata keeps its own position and reaches Fleet through a durable follow-up.

use crate::{
    CallerKind, CopyDirectorySummary, CopyKind, FleetKind, ObjectAddress, ObjectHost,
    ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    fleet_record_codec::{decode_json, required},
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, StoreError, storage::TransactionScope,
};
use fleet_sync_protocol::DatabaseId;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

#[derive(Serialize, Deserialize)]
pub(crate) struct DirectorySummaryUpdate;
impl CallTarget for DirectorySummaryUpdate {
    const KIND: &'static str = "central.directory-summary";
    type Request = CopyDirectorySummary;
    type Answer = ();
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
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
    pub(crate) async fn directory_summary_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        let request: CopyDirectorySummary = serde_json::from_value(call.request)
            .map_err(crate::object_call_envelope::encode_refusal)?;
        let _: DirectorySummaryUpdate = serde_json::from_value(call.target)
            .map_err(crate::object_call_envelope::encode_refusal)?;
        if !matches!(call.authority, CallAuthority::Object)
            || call.source_face != CallerKind::Copy
            || call.source != ObjectAddress::Copy(request.database)
        {
            return Err(CallFailure::NotSent {
                reason: durable_steps::NotSentReason::Unreachable { last_seen: None },
            });
        }
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Err(CallFailure::AnswerLost);
        };
        StepDriver::new(host.clone())
            .run(StoreDirectorySummary(request))
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&()).map_err(|_| CallFailure::AnswerLost)
    }
}

struct StoreDirectorySummary(CopyDirectorySummary);
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for StoreDirectorySummary
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = ();
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<(), Infallible>> {
        Box::pin(async move {
            let id = self.0.database.as_uuid().to_string();
            let row =
                crate::generated_fleet_record_reads::read_directory_summary(scope, id.clone())
                    .await
                    .map_err(StoreError::backend)?
                    .ok_or_else(|| corrupt("directory entry has not been created"))?;
            if required(row.project_id, "directory.project_id")? != id {
                return Err(corrupt("directory identity differs"));
            }
            if let Some(record) = row.copy_summary {
                let previous: CopyDirectorySummary =
                    decode_json(&record, "directory.copy_summary")?;
                if previous.database != self.0.database {
                    return Err(corrupt("stored summary database differs"));
                }
                if previous.position > self.0.position || previous == self.0 {
                    return Ok(((), Vec::new()));
                }
                if previous.position == self.0.position {
                    return Err(corrupt("one copy position has conflicting summaries"));
                }
            }
            crate::generated_fleet_record_writes::store_directory_summary(
                scope,
                serde_json::to_string(&self.0).map_err(StoreError::backend)?,
                id,
            )
            .await
            .map_err(StoreError::backend)?;
            Ok(((), Vec::new()))
        })
    }
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
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub async fn copy_summary(&self) -> Result<Option<CopyDirectorySummary>, StoreError> {
        let ObjectWork::Copy { host, database, .. } = &self.work else {
            return Err(corrupt("summary read reached another face"));
        };
        let (summary, _) = StepDriver::new(host.clone())
            .run(ReadCopySummary(*database))
            .await?;
        Ok(summary)
    }
}
struct ReadCopySummary(DatabaseId);
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadCopySummary
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<CopyDirectorySummary>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Infallible>> {
        Box::pin(async move {
            let mut rows = crate::generated_copy_summary::read_copy_summary(scope)
                .await
                .map_err(StoreError::backend)?;
            if rows.len() > 1 {
                return Err(corrupt("copy contains multiple summaries"));
            }
            let summary = if let Some(row) = rows.pop() {
                let summary: CopyDirectorySummary =
                    decode_json(&required(row.record, "copy.summary")?, "copy.summary")?;
                if required(row.database_id, "copy.summary.database")?
                    != self.0.as_uuid().to_string()
                    || summary.database != self.0
                {
                    return Err(corrupt("stored copy summary database differs"));
                }
                Some(summary)
            } else {
                None
            };
            Ok((summary, Vec::new()))
        })
    }
}
fn corrupt(message: &str) -> StoreError {
    StoreError::Corrupt {
        field: "copy.summary".into(),
        message: message.into(),
    }
}
