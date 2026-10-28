//! The authenticated Session supplies the installation; Fleet supplies report time.
use crate::{
    CallerKind, FleetKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority, fleet_archive_step::ReportArchive,
    object_call_envelope::ObjectCallEnvelope, object_runtime::ObjectWork, step_driver::StepDriver,
};
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, NotSentReason, storage::TransactionScope,
};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Serialize, Deserialize)]
pub(crate) struct FleetArchiveReport;
impl CallTarget for FleetArchiveReport {
    const KIND: &'static str = "central.archive-report";
    type Request = board_sync_protocol::ArchiveSeen;
    type Answer = fleet_admission::ArchiveReportOutcome;
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
    pub(crate) async fn archive_report_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source_face != CallerKind::Session
            || !matches!(call.authority, CallAuthority::Object)
        {
            return Err(refused());
        }
        let ObjectAddress::Connection(installation) = call.source else {
            return Err(refused());
        };
        let _: FleetArchiveReport = serde_json::from_value(call.target).map_err(|_| refused())?;
        let request: board_sync_protocol::ArchiveSeen =
            serde_json::from_value(call.request).map_err(|_| refused())?;
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(refused());
        };
        let report = fleet_admission::ArchiveReport {
            board: request.board,
            project: request.project,
            position: request.position,
            installation,
            reported_at: chrono::DateTime::from_timestamp_millis(io.now().unix_millis())
                .ok_or(CallFailure::AnswerLost)?,
        };
        let (answer, _) = StepDriver::new(host.clone())
            .run(ReportArchive {
                report,
                row_ids: crate::fleet_row_ids::FleetRowIds::new(io.clone()),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
