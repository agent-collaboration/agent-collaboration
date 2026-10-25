//! Owner paging reads one bounded set of canonical directory identities in one step.
use crate::{
    CallerKind, FleetKind, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    owner_operation_calls::{CopyDirectoryPage, CopyPageRequest, ReadCopyDirectoryPage},
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, CallFailure, StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;
impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String: Type<Host::Database> + Encode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Decode<'value, Host::Database> + Encode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn owner_directory_page(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source_face != CallerKind::Edge
            || !matches!(call.authority, CallAuthority::Owner(_))
        {
            return Err(CallFailure::AnswerLost);
        }
        let _: ReadCopyDirectoryPage = serde_json::from_value(call.target)
            .map_err(crate::object_call_envelope::encode_refusal)?;
        let request = serde_json::from_value(call.request)
            .map_err(crate::object_call_envelope::encode_refusal)?;
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(CallFailure::AnswerLost);
        };
        let as_of = chrono::DateTime::from_timestamp_millis(io.now().unix_millis())
            .ok_or(CallFailure::AnswerLost)?;
        let (page, _) = StepDriver::new(host.clone())
            .run(ReadDirectoryPage { request, as_of })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&page).map_err(|_| CallFailure::AnswerLost)
    }
}
struct ReadDirectoryPage {
    request: CopyPageRequest,
    as_of: chrono::DateTime<chrono::Utc>,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadDirectoryPage
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = CopyDirectoryPage;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let keys = crate::generated_fleet_record_reads::read_directory_key_page(
                scope,
                self.request.after.map(|after| after.as_str().to_owned()),
                u16::from(self.request.limit).to_string(),
            )
            .await
            .map_err(StoreError::backend)?;
            let projects = crate::fleet_directory_read::records_from_keys(
                scope,
                keys,
                &Default::default(),
                self.as_of,
            )
            .await?;
            let next_after = projects.last().map(|entry| entry.project.clone());
            Ok((
                CopyDirectoryPage {
                    projects,
                    next_after,
                    as_of: self.as_of,
                },
                Vec::new(),
            ))
        })
    }
}
