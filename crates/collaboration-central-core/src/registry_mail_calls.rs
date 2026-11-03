//! Fleet commits mail custody before answering the authenticated submitter.
use crate::FleetKind;

use crate::{
    CallerKind, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    mail_submit_admission::AdmitMailSubmit,
    mail_submit_calls::{MailSubmitRegistry, MailSubmitRequest},
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    step_driver::StepDriver,
};
use durable_steps::{AtomicStore, CallFailure, NotSentReason, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

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
    pub(crate) async fn mail_submit_call(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Session
            || !matches!(invocation.authority, CallAuthority::Machine { .. })
        {
            return Err(refused());
        }
        let CallAuthority::Machine {
            installation: source,
        } = invocation.authority
        else {
            return Err(refused());
        };
        let _: MailSubmitRegistry =
            serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let request: MailSubmitRequest =
            serde_json::from_value(invocation.request).map_err(|_| refused())?;
        let ObjectWork::Fleet {
            host, io, delivery, ..
        } = &self.work
        else {
            return Err(refused());
        };
        let (answer, _) = StepDriver::new(host.clone())
            .run(AdmitMailSubmit {
                source,
                request,
                limits: crate::central_mailbox_limits().map_err(|_| CallFailure::AnswerLost)?,
                relay: delivery.relay_peer,
                now: io.now(),
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
