//! Retired Wake wire requests cannot create timers; only old sender receipts remain readable.
use super::{
    CarrierAcceptance, DeliveryItem, DeliveryReceipt, LocalDelivery, MailConsumer,
    MailDeliveryRuntime, RouterDeliveryTypes,
};
use collaboration_mail_protocol::{
    RemoteWakeMutationState, RemoteWakeRefusal, RemoteWakeResult, WakeBody,
};
use fleet_sync_delivery::{LocalOutcome, OutboxStatusState};
use fleet_sync_protocol::{DeliveryStatus, ItemId};

impl MailDeliveryRuntime {
    /// Interim read-only recovery; remove after the 32-day retention following release.
    pub(crate) async fn recorded_wake_answer(
        &self,
        operation: &collaboration_protocol::OperationId,
    ) -> Result<Option<RemoteWakeResult>, ()> {
        let id = ItemId::new(uuid::Uuid::parse_str(operation.as_str()).map_err(|_| ())?)
            .map_err(|_| ())?;
        match self.outbox_status(id).await.map_err(|_| ())? {
            OutboxStatusState::Known {
                status:
                    DeliveryStatus::Delivered {
                        acceptance: CarrierAcceptance::Wake(answer),
                    },
                ..
            } => Ok(Some(*answer)),
            OutboxStatusState::Known {
                status: DeliveryStatus::Delivered { .. },
                ..
            } => Err(()),
            _ => Ok(None),
        }
    }
}

impl MailConsumer {
    pub(super) fn deliver_wake(
        &self,
        _item: &DeliveryItem<RouterDeliveryTypes>,
        _body: &WakeBody,
    ) -> LocalDelivery<RouterDeliveryTypes> {
        // The SDK retains its own prior acceptance before re-entering this consumer.
        // First-seen legacy requests receive a terminal refusal, with no Automation write.
        LocalDelivery {
            outcome: LocalOutcome::Delivered {
                acceptance: CarrierAcceptance::Wake(Box::new(RemoteWakeResult::Refused {
                    reason: RemoteWakeRefusal::AutomationUnavailable,
                    mutation: RemoteWakeMutationState::None,
                })),
            },
            observation: DeliveryReceipt {
                outcome: collaboration_protocol::DeliveryOutcome::NotSubmitted {
                    retryable: false,
                    reason: "Remote Wake requests are retired; use the central catalog.".into(),
                },
                reachability: None,
                client: None,
            }
            .into(),
        }
    }
}
