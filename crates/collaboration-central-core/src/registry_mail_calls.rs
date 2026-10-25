//! Registry mail admission releases its transaction before the mailbox call.

use crate::{
    CallerKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo, RegistryKind,
    caller_context::CallAuthority,
    mail_submit_admission::{AdmitMailSubmit, MailSubmitAdmission},
    mail_submit_calls::{MailSubmitRegistry, MailSubmitReply, MailSubmitRequest},
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    step_driver::StepDriver,
};
use board_sync_protocol::MailSubmitAnswer;
use durable_steps::{
    AtomicStore, CallFailure, Host as _, NotSentReason, storage::TransactionScope,
};
use fleet_sync_protocol::DeliveryAnswer;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

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
    pub(crate) async fn mail_submit_call(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Session
            || !matches!(invocation.authority, CallAuthority::Object)
        {
            return Err(refused());
        }
        let ObjectAddress::Connection(source) = invocation.source else {
            return Err(refused());
        };
        let _: MailSubmitRegistry =
            serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let request: MailSubmitRequest =
            serde_json::from_value(invocation.request).map_err(|_| refused())?;
        let ObjectWork::Registry {
            host, shared_host, ..
        } = &self.work
        else {
            return Err(refused());
        };
        let (admission, _) = StepDriver::new(host.clone())
            .run(AdmitMailSubmit { source, request })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        let answer = match admission {
            MailSubmitAdmission::Reply(reply) => reply,
            MailSubmitAdmission::Submit { target, request } => {
                let answer = shared_host
                    .dispatch(target, request, invocation.identity)
                    .await?;
                let answer = match answer {
                    DeliveryAnswer::Submit(result) => MailSubmitAnswer::Submitted(result),
                    DeliveryAnswer::MailboxUnavailable { reason } => {
                        MailSubmitAnswer::MailboxUnavailable { reason }
                    }
                    DeliveryAnswer::Fetch(_)
                    | DeliveryAnswer::Received(_)
                    | DeliveryAnswer::Report(_)
                    | DeliveryAnswer::StatusFetch(_)
                    | DeliveryAnswer::StatusAck(_)
                    | DeliveryAnswer::Query(_)
                    | DeliveryAnswer::Retire(_)
                    | DeliveryAnswer::Settle(_) => return Err(CallFailure::AnswerLost),
                };
                MailSubmitReply::Answer(answer)
            }
        };
        serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
    }
}

fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
