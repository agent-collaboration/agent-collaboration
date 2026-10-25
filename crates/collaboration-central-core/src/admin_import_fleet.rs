//! Only a copy's publication receipt can advance a reserved import to published.

use crate::{
    CallerKind, FleetKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    admin_import_contract::AdminImportRecord,
    admin_import_store::{AdminImportCommand, AdminImportRefusal, AdminImportStep},
    caller_context::CallAuthority,
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    step_driver::StepDriver,
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
pub(crate) struct AdminImportWorkflow;
impl CallTarget for AdminImportWorkflow {
    const KIND: &'static str = "central.admin-import-workflow";
    type Request = AdminImportCommand;
    type Answer = Result<AdminImportRecord, AdminImportRefusal>;
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
    pub(crate) async fn admin_import_workflow(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        let _: AdminImportWorkflow = serde_json::from_value(call.target).map_err(|_| refused())?;
        let command: AdminImportCommand =
            serde_json::from_value(call.request).map_err(|_| refused())?;
        let allowed = match &command {
            AdminImportCommand::Published { .. } => {
                call.source_face == CallerKind::Copy
                    && matches!(call.authority, CallAuthority::Object)
                    && matches!(&call.source, ObjectAddress::Copy(project)
                        if *project == command.database())
            }
            _ => {
                call.source_face == CallerKind::Edge
                    && call.source == ObjectAddress::Registry
                    && matches!(call.authority, CallAuthority::Owner(_))
            }
        };
        if !allowed {
            return Err(refused());
        }
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Err(refused());
        };
        let (answer, _) = StepDriver::new(host.clone())
            .run(AdminImportStep(command))
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
