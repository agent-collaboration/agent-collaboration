//! Fleet decodes Automation calls after the Session owner binds the installation.
use crate::{
    CallerKind, FleetKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    automation_catalog_step::AutomationCatalogStep, caller_context::CallAuthority,
    object_call_envelope::ObjectCallEnvelope, object_runtime::ObjectWork, step_driver::StepDriver,
};
use board_sync_protocol::BoardPayload;
use collaboration_protocol::AutomationAnswer;
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, NotSentReason, storage::TransactionScope,
};
use message_board::ServiceId;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Serialize, Deserialize)]
pub(crate) struct FleetAutomation;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FleetAutomationRequest {
    pub installation: ServiceId,
    pub call: BoardPayload,
}

impl CallTarget for FleetAutomation {
    const KIND: &'static str = "central.automation";
    type Request = FleetAutomationRequest;
    type Answer = AutomationAnswer;
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
    pub(crate) async fn automation_call(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Session
            || !matches!(invocation.authority, CallAuthority::Object)
        {
            return Err(refused());
        }
        let _: FleetAutomation =
            serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let request: FleetAutomationRequest =
            serde_json::from_value(invocation.request).map_err(|_| refused())?;
        if invocation.source != ObjectAddress::Connection(request.installation.clone()) {
            return Err(refused());
        }
        let call = match crate::automation_request_decode::decode(request.call) {
            Ok(call) => call,
            Err(error) => {
                let answer = error.into_answer().map_err(|_| CallFailure::AnswerLost)?;
                return serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost);
            }
        };
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(refused());
        };
        let (answer, _) = StepDriver::new(host.clone())
            .run(AutomationCatalogStep {
                call,
                installation: request.installation,
                now_ms: io.now().unix_millis(),
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
