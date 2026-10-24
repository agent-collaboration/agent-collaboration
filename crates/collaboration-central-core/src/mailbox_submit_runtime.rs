//! Registry-verified mail enters custody in one step; health follows its final result.
use crate::{
    CallerKind, MailboxKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    mail_submit_calls::{MailboxSubmission, MailboxSubmit},
    mailbox_step::ActivateMailboxWork,
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepResult},
};
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, JournalScope, RequestKey, storage::TransactionScope,
};
use fleet_sync_delivery::{
    DeliveryLimits, Mailbox, MailboxOperation, PublishStatus, admit_publications, delivery_rollback,
};
use fleet_sync_protocol::{DeliveryAnswer, PeerId};
use runtime_primitives::{Clock, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

type Answer = DeliveryAnswer<BoardDeliveryTypes>;
struct SubmitMailbox {
    recipient: PeerId,
    submission: MailboxSubmission,
    limits: DeliveryLimits,
    now: SyncTime,
}
impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<MailboxKind, Host, Io>
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
    pub(crate) async fn mailbox_submit_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source != ObjectAddress::Registry
            || call.source_face != CallerKind::Registry
            || !matches!(call.authority, CallAuthority::Object)
        {
            return Err(refused());
        }
        let target: MailboxSubmit = serde_json::from_value(call.target)
            .map_err(crate::object_call_envelope::encode_refusal)?;
        let submission: MailboxSubmission = serde_json::from_value(call.request)
            .map_err(crate::object_call_envelope::encode_refusal)?;
        let ObjectWork::Mailbox {
            shared_host,
            installation,
            limits,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        if target.installation != *installation
            || submission.item.recipient().peer != target.recipient
            || submission.item.recipient().address.endpoint.service_id != *installation
        {
            return Err(refused());
        }
        self.run_mailbox_step(
            target.recipient,
            MailboxOperation::Submit,
            SubmitMailbox {
                recipient: target.recipient,
                submission,
                limits: *limits,
                now: shared_host.now(),
            },
        )
        .await
    }
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for SubmitMailbox
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Answer;
    type Effect = ActivateMailboxWork;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Answer, Self::Effect>> {
        Box::pin(async move {
            let stream = self.submission.item.stream();
            let item_id = self.submission.item.id();
            let installation = self
                .submission
                .item
                .recipient()
                .address
                .endpoint
                .service_id
                .clone();
            let (answer, admitted, notice_owed) = async {
                let answer =
                    Mailbox::<_, BoardDeliveryTypes>::new(scope, self.recipient, self.limits)
                        .submit(
                            self.submission.origin,
                            *self.submission.item,
                            self.now,
                            self.submission.route,
                            self.submission.rank,
                        )
                        .await?;
                let notice_owed = matches!(
                    &answer,
                    fleet_sync_protocol::SubmitResult::Accepted { .. }
                        | fleet_sync_protocol::SubmitResult::Replayed { .. }
                );
                if notice_owed {
                    scope
                        .admit::<crate::machine_notice_outbox::MailboxNoticeTask<Store>>(
                            &RequestKey::new(format!("mail-waiting:{}", item_id.as_uuid()))
                                .map_err(|error| {
                                    fleet_sync_delivery::DeliveryError::Storage(error.to_string())
                                })?,
                            &crate::machine_notice_outbox::MachineNoticeInput {
                                installation,
                                notice: board_sync_protocol::BoardNotice::MailWaiting { stream },
                            },
                        )
                        .await
                        .map_err(|error| {
                            fleet_sync_delivery::DeliveryError::Storage(error.to_string())
                        })?;
                    scope.ensure_wake(self.now).await.map_err(|error| {
                        fleet_sync_delivery::DeliveryError::Storage(error.to_string())
                    })?;
                }
                let admitted = admit_publications::<
                    _,
                    BoardDeliveryTypes,
                    PublishStatus<Store, Driver, BoardDeliveryTypes>,
                >(scope)
                .await?;
                Ok::<_, fleet_sync_delivery::DeliveryError>((
                    Answer::Submit(answer),
                    admitted,
                    notice_owed,
                ))
            }
            .await
            .map_err(delivery_rollback)?;
            let effects = if admitted.is_empty() && !notice_owed {
                Vec::new()
            } else {
                vec![ActivateMailboxWork]
            };
            Ok((answer, effects))
        })
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: durable_steps::NotSentReason::Unreachable { last_seen: None },
    }
}
