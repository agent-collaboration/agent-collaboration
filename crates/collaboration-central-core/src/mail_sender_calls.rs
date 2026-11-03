//! Sender query, retirement and settlement use the authenticated Registry mapping.
use crate::FleetKind;
use crate::{
    CallerKind, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    mailbox_requests::{MailboxRelayRequest, MailboxRequest},
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
    registry_peer_lookup::service_for_peer,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, Host as _, NotSentReason,
    storage::TransactionScope,
};
use fleet_sync_protocol::{DeliveryAnswer, DeliveryRefusal, DeliveryRequest};
use fleet_sync_session::SessionKey;
use message_board::ServiceId;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

#[derive(Serialize, Deserialize)]
pub(crate) struct SenderMailboxRegistry;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SenderMailboxRequest {
    pub session: SessionKey,
    pub request: Box<DeliveryRequest<BoardDeliveryTypes>>,
}
impl CallTarget for SenderMailboxRegistry {
    const KIND: &'static str = "central.sender-mailbox";
    type Request = SenderMailboxRequest;
    type Answer = DeliveryAnswer<BoardDeliveryTypes>;
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
    pub(crate) async fn sender_mailbox_call(
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
        let _: SenderMailboxRegistry =
            serde_json::from_value(invocation.target).map_err(encode_refusal)?;
        let request: SenderMailboxRequest =
            serde_json::from_value(invocation.request).map_err(encode_refusal)?;
        if request.request.call() != &invocation.identity {
            return Err(refused());
        }
        let ObjectWork::Fleet {
            host,
            shared_host,
            io,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        let (route, _) = StepDriver::new(host.clone())
            .run(SenderMailboxRoute {
                source,
                input: request,
                limits: crate::central_mailbox_limits().map_err(|_| CallFailure::AnswerLost)?,
                now: io.now(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        let answer = match route {
            SenderRoute::Refused => return Err(refused()),
            SenderRoute::UnknownRecipient => DeliveryAnswer::MailboxUnavailable {
                reason: DeliveryRefusal::RecipientUnknown,
            },
            SenderRoute::Answered(answer) => answer,
            SenderRoute::Mailbox { target, request } => {
                shared_host
                    .dispatch(target, request, invocation.identity)
                    .await?
            }
        };
        serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
    }
}
enum SenderRoute {
    Refused,
    UnknownRecipient,
    Answered(DeliveryAnswer<BoardDeliveryTypes>),
    Mailbox {
        target: MailboxRequest,
        request: MailboxRelayRequest,
    },
}
struct SenderMailboxRoute {
    source: ServiceId,
    input: SenderMailboxRequest,
    limits: fleet_sync_delivery::DeliveryLimits,
    now: runtime_primitives::SyncTime,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for SenderMailboxRoute
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = SenderRoute;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            if service_for_peer(scope, self.input.session.peer)
                .await?
                .as_ref()
                != Some(&self.source)
            {
                return Ok((SenderRoute::Refused, Vec::new()));
            }
            if let DeliveryRequest::Query { items, .. } = self.input.request.as_ref() {
                let mut statuses = Vec::with_capacity(items.len());
                let mut outbox =
                    fleet_sync_delivery::Outbox::<_, BoardDeliveryTypes>::new(scope, self.limits);
                for item in items {
                    match outbox.relay_query(self.input.session.peer, *item).await {
                        Ok(status) => statuses.push(status),
                        Err(fleet_sync_delivery::DeliveryError::Refused { reason }) => {
                            return Ok((
                                SenderRoute::Answered(DeliveryAnswer::MailboxUnavailable {
                                    reason,
                                }),
                                Vec::new(),
                            ));
                        }
                        Err(error) => return Err(fleet_sync_delivery::delivery_rollback(error)),
                    }
                }
                return Ok((
                    SenderRoute::Answered(DeliveryAnswer::Query(
                        fleet_sync_protocol::QueryResult { statuses },
                    )),
                    Vec::new(),
                ));
            }
            if let DeliveryRequest::Settle { item, digest, .. } = self.input.request.as_ref() {
                let answer =
                    fleet_sync_delivery::Outbox::<_, BoardDeliveryTypes>::new(scope, self.limits)
                        .relay_settle::<crate::fleet_mail_tasks::FleetMailRemoval<Store, Driver>>(
                            self.input.session.peer,
                            *item,
                            digest.clone(),
                            self.now,
                        )
                        .await;
                let answer = match answer {
                    Ok(result) => DeliveryAnswer::Settle(result),
                    Err(fleet_sync_delivery::DeliveryError::Refused { reason }) => {
                        DeliveryAnswer::MailboxUnavailable { reason }
                    }
                    Err(error) => return Err(fleet_sync_delivery::delivery_rollback(error)),
                };
                return Ok((SenderRoute::Answered(answer), Vec::new()));
            }
            let mailbox = match self.input.request.as_ref() {
                DeliveryRequest::Query { mailbox, .. }
                | DeliveryRequest::Settle { mailbox, .. }
                | DeliveryRequest::Retire { mailbox, .. } => *mailbox,
                DeliveryRequest::Submit { .. }
                | DeliveryRequest::Fetch { .. }
                | DeliveryRequest::Received { .. }
                | DeliveryRequest::Report { .. }
                | DeliveryRequest::StatusFetch { .. }
                | DeliveryRequest::StatusAck { .. } => {
                    return Ok((SenderRoute::Refused, Vec::new()));
                }
            };
            let Some(installation) = service_for_peer(scope, mailbox).await? else {
                return Ok((SenderRoute::UnknownRecipient, Vec::new()));
            };
            Ok((
                SenderRoute::Mailbox {
                    target: MailboxRequest {
                        installation,
                        recipient: mailbox,
                    },
                    request: MailboxRelayRequest {
                        actor: self.input.session.peer,
                        request: self.input.request,
                    },
                },
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
