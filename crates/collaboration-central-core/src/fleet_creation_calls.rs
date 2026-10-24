//! Registry reads the first creation decision; admission stays in Registry.
use crate::{
    CallerKind, FleetKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, NotSentReason, storage::TransactionScope,
};
use fleet_admission::ProjectCreationDecision;
use message_board::ProjectId;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

#[derive(Serialize, Deserialize)]
pub(crate) struct ReadProjectCreationDecision;
impl CallTarget for ReadProjectCreationDecision {
    const KIND: &'static str = "central.creation-decision";
    type Request = ProjectId;
    type Answer = Option<ProjectCreationDecision>;
}
impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>: Type<Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn creation_decision_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source != ObjectAddress::Registry
            || call.source_face != CallerKind::Registry
            || !matches!(call.authority, CallAuthority::Object)
        {
            return Err(refused());
        }
        let _: ReadProjectCreationDecision =
            serde_json::from_value(call.target).map_err(|_| refused())?;
        let project: ProjectId = serde_json::from_value(call.request).map_err(|_| refused())?;
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Err(refused());
        };
        let (decision, _) = StepDriver::new(host.clone())
            .run(ReadCreation { project })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&decision).map_err(|_| CallFailure::AnswerLost)
    }
}
struct ReadCreation {
    project: ProjectId,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadCreation
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<ProjectCreationDecision>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            Ok((
                crate::fleet_creation_store::read(scope, &self.project).await?,
                Vec::new(),
            ))
        })
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
