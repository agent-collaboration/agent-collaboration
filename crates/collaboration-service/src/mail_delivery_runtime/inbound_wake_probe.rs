//! Feature-gated entry and admission observation for real Host startup contract tests.
use super::{LocalDelivery, MailDeliveryRuntime, RouterDeliveryTypes};
use collaboration_mail_protocol::WakeBody;
use fleet_sync_protocol::DeliveryItem;

impl MailDeliveryRuntime {
    /// Drives the actual inbound consumer, including its retired Wake-request refusal.
    pub fn test_support_deliver_inbound_wake(
        &self,
        item: &DeliveryItem<RouterDeliveryTypes>,
        body: &WakeBody,
    ) -> LocalDelivery<RouterDeliveryTypes> {
        self.consumer.deliver_wake(item, body)
    }
}
