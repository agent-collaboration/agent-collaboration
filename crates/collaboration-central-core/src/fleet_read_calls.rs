//! Fleet reads stay inside the canonical Fleet owner after leaving the session.
use crate::{
    CallerKind, FleetKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority, fleet_read_step::ReadFleet,
    object_call_envelope::ObjectCallEnvelope, object_runtime::ObjectWork, step_driver::StepDriver,
};
use board_sync_protocol::{BoardReadPage, FleetSnapshotCursor};
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, NotSentReason, storage::TransactionScope,
};
use message_board::FleetPosition;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Serialize, Deserialize)]
pub(crate) struct FleetRead;
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum FleetReadRequest {
    Feed {
        after: FleetPosition,
    },
    Snapshot {
        cursor: Option<FleetSnapshotCursor>,
    },
    Directory {
        query: fleet_admission::DirectoryQuery,
    },
}
impl CallTarget for FleetRead {
    const KIND: &'static str = "central.fleet-read";
    type Request = FleetReadRequest;
    type Answer = BoardReadPage;
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
    pub(crate) async fn fleet_read_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source_face != CallerKind::Session
            || !matches!(call.source, ObjectAddress::Connection(_))
            || !matches!(call.authority, CallAuthority::Object)
        {
            return Err(refused());
        }
        let ObjectAddress::Connection(installation) = call.source else {
            return Err(refused());
        };
        let _: FleetRead = serde_json::from_value(call.target).map_err(|_| refused())?;
        let request: FleetReadRequest =
            serde_json::from_value(call.request).map_err(|_| refused())?;
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(refused());
        };
        let as_of = chrono::DateTime::from_timestamp_millis(io.now().unix_millis())
            .ok_or(CallFailure::AnswerLost)?;
        let (answer, _) = StepDriver::new(host.clone())
            .run(ReadFleet {
                request,
                as_of,
                installation,
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
