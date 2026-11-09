//! Fleet validates mail authority and admits custody in the same transaction.

use crate::{
    mail_submit_calls::{MailSubmitReply, MailSubmitRequest},
    step_driver::{StepCallback, StepResult},
};
use board_sync_protocol::{BoardConsumerRequest, BoardDeliveryTypes, MailSubmitAnswer};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use fleet_mail_delivery::{DeliveryLimits, Outbox};
use fleet_protocol::{DatabaseId, DeliveryRefusal, SubmitResult};
use message_board::ServiceId;
use runtime_primitives::Clock;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

#[cfg(test)]
#[path = "mail_submit_admission_test.rs"]
mod tests;

pub(crate) struct AdmitMailSubmit {
    pub source: ServiceId,
    pub request: MailSubmitRequest,
    pub limits: DeliveryLimits,
    pub relay: fleet_protocol::PeerId,
    pub clock: std::sync::Arc<dyn Clock>,
}

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for AdmitMailSubmit
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = MailSubmitReply;
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
            let now = self.clock.now();
            let Some(sender) = crate::machine_enrollment::admission::read_current_enrolled_machine(
                scope,
                session.peer,
                now,
            )
            .await?
            else {
                return reply(MailSubmitReply::Answer(MailSubmitAnswer::Submitted(
                    SubmitResult::Refused {
                        reason: DeliveryRefusal::SenderNotEnrolled,
                    },
                )));
            };
            if origin.installation != self.source
                || sender.origin.installation != self.source
                || !item
                    .sender()
                    .matches_submitting_installation(&origin.installation)
            {
                return reply(MailSubmitReply::InvalidShape {
                    field: "sender".into(),
                });
            }
            let recipient = item.recipient().peer;
            let mut recipients = crate::generated_enrollment::read_enrollments(
                scope,
                None,
                None,
                None,
                Some(recipient.to_string()),
            )
            .await
            .map_err(StoreError::backend)?;
            if recipients.len() > 1 {
                return Err(crate::machine_enrollment_record::corrupt("peer_id"));
            }
            let Some(row) = recipients.pop() else {
                return reply(MailSubmitReply::Answer(MailSubmitAnswer::Submitted(
                    SubmitResult::Refused {
                        reason: DeliveryRefusal::RecipientUnknown,
                    },
                )));
            };
            let recipient_machine = crate::machine_enrollment_record::StoredMachine::decode(row)?;
            if recipient_machine.peer != recipient {
                return Err(crate::machine_enrollment_record::corrupt("peer_id"));
            }
            let recipient_revoked = recipient_machine.state
                == crate::machine_enrollment_record::EnrollmentState::Revoked;
            let installation = recipient_machine.origin.installation;
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
            let submitter = match (item.sender(), origin_epoch) {
                (collaboration_mail_protocol::MailSender::Project { project }, Some(epoch)) => {
                    let database = DatabaseId::try_from(project.as_str().to_owned())
                        .map_err(StoreError::backend)?;
                    let grant = fleet_project_lease::epoch_holder(scope, database, epoch)
                        .await
                        .map_err(StoreError::backend)?;
                    if !grant.is_some_and(|grant| grant.holder == session.peer) {
                        return reply(MailSubmitReply::Answer(MailSubmitAnswer::NotWriter));
                    }
                    fleet_protocol::Submitter::Project(database)
                }
                (
                    collaboration_mail_protocol::MailSender::Session(_)
                    | collaboration_mail_protocol::MailSender::HumanUser { .. },
                    None,
                ) => fleet_protocol::Submitter::Machine(session.peer),
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
            let mut outbox = Outbox::<_, BoardDeliveryTypes, _>::new(
                scope,
                self.limits,
                &crate::fleet_sender_binding::FleetSenderBinding,
            );
            if recipient_revoked
                && !fleet_mail_delivery::item_is_too_old(item.id(), now)
                && matches!(
                    outbox
                        .status(item.id())
                        .await
                        .map_err(fleet_mail_delivery::delivery_rollback)?
                        .state,
                    fleet_mail_delivery::OutboxStatusState::Unknown,
                )
            {
                return reply(MailSubmitReply::Answer(MailSubmitAnswer::Submitted(
                    SubmitResult::Refused {
                        reason: DeliveryRefusal::RecipientRevoked,
                    },
                )));
            }
            // Existing immutable custody still goes through the SDK's replay
            // and conflict rules, including while the recipient is revoked.
            let answer = outbox
                .submit_relay::<crate::central_mail_sink::CentralMailSink>(
                    submitter,
                    *item,
                    self.relay,
                    origin.installation,
                    now,
                )
                .await
                .map_err(fleet_mail_delivery::delivery_rollback)?;
            Ok((
                MailSubmitReply::Answer(MailSubmitAnswer::Submitted(answer)),
                Vec::new(),
            ))
        })
    }
}

fn reply(value: MailSubmitReply) -> StepResult<MailSubmitReply, Infallible> {
    Ok((value, Vec::new()))
}
