//! Sender reconciliation reads and settles Fleet custody under authenticated authority.
use crate::{
    registry_peer_lookup::service_for_peer,
    step_driver::{StepCallback, StepResult},
};
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{AtomicStore, BoxFuture, CallTarget, storage::TransactionScope};
use fleet_protocol::{DeliveryAnswer, DeliveryRequest};
use fleet_session::SessionKey;
use message_board::ServiceId;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

#[cfg(test)]
#[path = "mail_sender_calls_test.rs"]
mod tests;
#[derive(Serialize, Deserialize)]
pub(crate) struct SenderFleetMail;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SenderMailRequest {
    pub session: SessionKey,
    pub request: Box<DeliveryRequest<BoardDeliveryTypes>>,
}
impl CallTarget for SenderFleetMail {
    const KIND: &'static str = "central.sender-mail";
    type Request = SenderMailRequest;
    type Answer = DeliveryAnswer<BoardDeliveryTypes>;
}

pub(crate) enum SenderRoute {
    Refused,
    Answered(DeliveryAnswer<BoardDeliveryTypes>),
}
pub(crate) struct AdmitSenderMail {
    pub source: ServiceId,
    pub input: SenderMailRequest,
    pub limits: fleet_mail_delivery::DeliveryLimits,
    pub now: runtime_primitives::SyncTime,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for AdmitSenderMail
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
                for item in items {
                    let Some(submitter) = crate::mail_sender_authority::authorized_submitter(
                        scope,
                        self.input.session.peer,
                        *item,
                        self.limits,
                        self.now,
                    )
                    .await?
                    else {
                        return Ok((
                            SenderRoute::Answered(DeliveryAnswer::DeliveryUnavailable {
                                reason: fleet_protocol::DeliveryRefusal::NotSubmitter,
                            }),
                            Vec::new(),
                        ));
                    };
                    match fleet_mail_delivery::Outbox::<_, BoardDeliveryTypes, _>::new(
                        scope,
                        self.limits,
                        &crate::fleet_sender_binding::FleetSenderBinding,
                    )
                    .relay_query(submitter, *item)
                    .await
                    {
                        Ok(status) => statuses.push(status),
                        Err(fleet_mail_delivery::DeliveryError::Refused { reason }) => {
                            return Ok((
                                SenderRoute::Answered(DeliveryAnswer::DeliveryUnavailable {
                                    reason,
                                }),
                                Vec::new(),
                            ));
                        }
                        Err(error) => return Err(fleet_mail_delivery::delivery_rollback(error)),
                    }
                }
                return Ok((
                    SenderRoute::Answered(DeliveryAnswer::Query(fleet_protocol::QueryResult {
                        statuses,
                    })),
                    Vec::new(),
                ));
            }
            if let DeliveryRequest::Settle {
                submitter,
                item,
                digest,
                ..
            } = self.input.request.as_ref()
            {
                if !crate::mail_sender_authority::claim_is_authorized(
                    scope,
                    self.input.session.peer,
                    *submitter,
                    self.now,
                )
                .await?
                {
                    return Ok((
                        SenderRoute::Answered(DeliveryAnswer::DeliveryUnavailable {
                            reason: fleet_protocol::DeliveryRefusal::NotSubmitter,
                        }),
                        Vec::new(),
                    ));
                };
                let answer = fleet_mail_delivery::Outbox::<_, BoardDeliveryTypes, _>::new(
                    scope,
                    self.limits,
                    &crate::fleet_sender_binding::FleetSenderBinding,
                )
                .relay_settle::<fleet_mail_delivery::JournaledRemoval<
                    crate::fleet_mail_tasks::FleetMailRemoval<Store, Driver>,
                >>(*submitter, *item, digest.clone(), self.now)
                .await;
                let answer = match answer {
                    Ok(result) => DeliveryAnswer::Settle(result),
                    Err(fleet_mail_delivery::DeliveryError::Refused { reason }) => {
                        DeliveryAnswer::DeliveryUnavailable { reason }
                    }
                    Err(error) => return Err(fleet_mail_delivery::delivery_rollback(error)),
                };
                return Ok((SenderRoute::Answered(answer), Vec::new()));
            }
            Ok((SenderRoute::Refused, Vec::new()))
        })
    }
}
