//! Checked relay calls delegate custody rules to the public library Mailbox.
use crate::{
    CallerKind, MailboxKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    mailbox_step::ActivateMailboxWork,
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepResult},
};
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{AtomicStore, BoxFuture, CallFailure, CallTarget, storage::TransactionScope};
use fleet_sync_delivery::{
    DeliveryLimits, Mailbox, MailboxOperation, PublishStatus, admit_publications, delivery_rollback,
};
use fleet_sync_protocol::{DeliveryAnswer, DeliveryRequest, PeerId};
use message_board::ServiceId;
use runtime_primitives::{Clock, SyncTime};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MailboxRequest {
    pub installation: ServiceId,
    pub recipient: PeerId,
}
impl CallTarget for MailboxRequest {
    const KIND: &'static str = "central.mailbox-request";
    type Request = MailboxRelayRequest;
    type Answer = DeliveryAnswer<BoardDeliveryTypes>;
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MailboxRelayRequest {
    pub actor: PeerId,
    pub request: Box<DeliveryRequest<BoardDeliveryTypes>>,
}
struct RelayMailbox {
    recipient: PeerId,
    input: MailboxRelayRequest,
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
    pub(crate) async fn mailbox_request_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        let target: MailboxRequest = serde_json::from_value(call.target).map_err(encode_refusal)?;
        let input: MailboxRelayRequest =
            serde_json::from_value(call.request).map_err(encode_refusal)?;
        let ObjectWork::Mailbox {
            installation,
            limits,
            shared_host,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        if target.installation != *installation
            || input.request.call() != &call.identity
            || !matches!(call.authority, CallAuthority::Object)
        {
            return Err(refused());
        }
        let operation = match input.request.as_ref() {
            DeliveryRequest::Fetch { .. } => MailboxOperation::Fetch,
            DeliveryRequest::Received { .. } => MailboxOperation::Received,
            DeliveryRequest::Report { .. } => MailboxOperation::Report,
            DeliveryRequest::Query { mailbox, .. } if *mailbox == target.recipient => {
                MailboxOperation::Query
            }
            DeliveryRequest::Retire { mailbox, .. } if *mailbox == target.recipient => {
                MailboxOperation::Retire
            }
            DeliveryRequest::Settle { mailbox, .. } if *mailbox == target.recipient => {
                MailboxOperation::Settle
            }
            // Submit requires Registry's epoch/provenance verification; status
            // streams belong to Session. Neither enters this relay call.
            _ => return Err(refused()),
        };
        let allowed = match (&call.source, call.source_face) {
            (ObjectAddress::Registry, CallerKind::Registry) => true,
            (ObjectAddress::Connection(service), CallerKind::Session) => {
                service == installation
                    && input.actor == target.recipient
                    && matches!(
                        operation,
                        MailboxOperation::Fetch
                            | MailboxOperation::Received
                            | MailboxOperation::Report
                    )
            }
            _ => false,
        };
        if !allowed {
            return Err(refused());
        }
        self.run_mailbox_step(
            target.recipient,
            operation,
            RelayMailbox {
                recipient: target.recipient,
                input,
                limits: *limits,
                now: shared_host.now(),
            },
        )
        .await
    }
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for RelayMailbox
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = DeliveryAnswer<BoardDeliveryTypes>;
    type Effect = ActivateMailboxWork;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let (answer, admitted) = async {
                let mut mailbox =
                    Mailbox::<_, BoardDeliveryTypes>::new(scope, self.recipient, self.limits);
                let answer = match *self.input.request {
                    DeliveryRequest::Fetch {
                        stream,
                        after,
                        max_items,
                        max_bytes,
                        inbox_room_bytes,
                        ..
                    } => DeliveryAnswer::Fetch(
                        mailbox
                            .fetch_with_budget(
                                self.input.actor,
                                fleet_sync_delivery::FetchInput {
                                    stream,
                                    after,
                                    max_items,
                                    max_bytes,
                                    inbox_room_bytes,
                                },
                                self.now,
                            )
                            .await?,
                    ),
                    DeliveryRequest::Received {
                        stream, through, ..
                    } => DeliveryAnswer::Received(
                        mailbox
                            .received(self.input.actor, stream, through, self.now)
                            .await?,
                    ),
                    DeliveryRequest::Report { report, .. } => {
                        DeliveryAnswer::Report(mailbox.report(self.input.actor, report).await?)
                    }
                    DeliveryRequest::Query { items, .. } => {
                        DeliveryAnswer::Query(mailbox.query(&items).await?)
                    }
                    DeliveryRequest::Retire { call, items, .. } => DeliveryAnswer::Retire(
                        mailbox.retire(self.input.actor, &call, &items).await?,
                    ),
                    DeliveryRequest::Settle { item, digest, .. } => {
                        DeliveryAnswer::Settle(mailbox.settle(item, digest).await?)
                    }
                    _ => return Err(fleet_sync_delivery::DeliveryError::InvalidBatch),
                };
                let admitted = admit_publications::<
                    _,
                    BoardDeliveryTypes,
                    PublishStatus<Store, Driver, BoardDeliveryTypes>,
                >(scope)
                .await?;
                Ok::<_, fleet_sync_delivery::DeliveryError>((answer, admitted))
            }
            .await
            .map_err(delivery_rollback)?;
            Ok((
                answer,
                if admitted.is_empty() {
                    Vec::new()
                } else {
                    vec![ActivateMailboxWork]
                },
            ))
        })
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: durable_steps::NotSentReason::Unreachable { last_seen: None },
    }
}
