//! A wake request from another installation is refused: this build keeps no local wakes.
//!
//! The mail protocol still carries `BoardDeliveryBody::Wake` until the board replication that
//! sends it leaves. Answering it with a refusal settles the sender's item instead of leaving it
//! retried and unanswered.
use super::{CarrierAcceptance, DeliveryItem, LocalDelivery, MailConsumer, RouterDeliveryTypes};
use collaboration_mail_protocol::{
    RemoteWakeMutationState, RemoteWakeRefusal, RemoteWakeResult, WakeBody,
};
use fleet_sync_delivery::LocalOutcome;

impl MailConsumer {
    pub(super) fn deliver_wake(
        &self,
        _item: &DeliveryItem<RouterDeliveryTypes>,
        _body: &WakeBody,
    ) -> LocalDelivery<RouterDeliveryTypes> {
        let response = RemoteWakeResult::Refused {
            reason: RemoteWakeRefusal::AutomationUnavailable,
            mutation: RemoteWakeMutationState::None,
        };
        LocalDelivery {
            outcome: LocalOutcome::Delivered {
                acceptance: CarrierAcceptance::Wake(Box::new(response)),
            },
            // Wake admission is an operation answer, never a fabricated native push.
            observation: collaboration_protocol::DeliveryReceipt {
                outcome: collaboration_protocol::DeliveryOutcome::NotSubmitted {
                    retryable: false,
                    reason: "owning wake operation answered".into(),
                },
                reachability: None,
                client: None,
            }
            .into(),
        }
    }
}
