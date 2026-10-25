//! The registry validates recipient identity and exact historical mail authority.

use crate::{
    mail_submit_calls::{MailSubmitReply, MailSubmitRequest, MailboxSubmission, MailboxSubmit},
    registry_peer_lookup::service_for_peer,
    step_driver::{StepCallback, StepResult},
};
use board_sync_protocol::{BoardConsumerRequest, MailSubmitAnswer};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{DatabaseId, DeliveryRefusal, SubmitResult};
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

pub(crate) struct AdmitMailSubmit {
    pub source: ServiceId,
    pub request: MailSubmitRequest,
}

pub(crate) enum MailSubmitAdmission {
    Reply(MailSubmitReply),
    Submit {
        target: MailboxSubmit,
        request: MailboxSubmission,
    },
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for AdmitMailSubmit
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = MailSubmitAdmission;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let MailSubmitRequest {
                session,
                origin,
                item,
                origin_epoch,
            } = self.request;
            if origin.installation != self.source
                || service_for_peer(scope, session.peer).await?.as_ref() != Some(&self.source)
                || !item
                    .sender()
                    .matches_submitting_installation(&origin.installation)
            {
                return reply(MailSubmitReply::InvalidShape {
                    field: "sender".into(),
                });
            }
            let recipient = item.recipient().peer;
            let Some(installation) = service_for_peer(scope, recipient).await? else {
                return reply(MailSubmitReply::Answer(MailSubmitAnswer::Submitted(
                    SubmitResult::Refused {
                        reason: DeliveryRefusal::RecipientUnknown,
                    },
                )));
            };
            // Both identities come from this one registry row. No ServiceId is
            // reinterpreted as a PeerId, including in a replay or cancel.
            let request = match BoardConsumerRequest::mail_submit(
                *item,
                origin_epoch,
                &installation,
                recipient,
            ) {
                Ok(request) => request,
                Err(error) => {
                    return reply(MailSubmitReply::InvalidShape {
                        field: error.field.into(),
                    });
                }
            };
            let BoardConsumerRequest::MailSubmit { item, origin_epoch } = request else {
                return Err(StoreError::Aborted {
                    reason: "mail constructor returned a different operation".into(),
                });
            };
            let rank = match (item.sender(), origin_epoch) {
                (collaboration_mail_protocol::MailSender::Project { project }, Some(epoch)) => {
                    let database = DatabaseId::try_from(project.as_str().to_owned())
                        .map_err(StoreError::backend)?;
                    let grant = fleet_sync_lease::epoch_holder(scope, database, epoch)
                        .await
                        .map_err(StoreError::backend)?;
                    if !grant.is_some_and(|grant| grant.holder == session.peer) {
                        return reply(MailSubmitReply::Answer(MailSubmitAnswer::NotWriter));
                    }
                    Some(epoch.value())
                }
                (
                    collaboration_mail_protocol::MailSender::Session(_)
                    | collaboration_mail_protocol::MailSender::HumanUser { .. },
                    None,
                ) => None,
                (collaboration_mail_protocol::MailSender::Project { .. }, None)
                | (
                    collaboration_mail_protocol::MailSender::Session(_)
                    | collaboration_mail_protocol::MailSender::HumanUser { .. },
                    Some(_),
                ) => {
                    return reply(MailSubmitReply::InvalidShape {
                        field: "originEpoch".into(),
                    });
                }
            };
            Ok((
                MailSubmitAdmission::Submit {
                    target: MailboxSubmit {
                        installation,
                        recipient,
                    },
                    request: MailboxSubmission {
                        origin: session.peer,
                        route: origin.installation,
                        rank,
                        item,
                    },
                },
                Vec::new(),
            ))
        })
    }
}

fn reply(value: MailSubmitReply) -> StepResult<MailSubmitAdmission, Infallible> {
    Ok((MailSubmitAdmission::Reply(value), Vec::new()))
}
