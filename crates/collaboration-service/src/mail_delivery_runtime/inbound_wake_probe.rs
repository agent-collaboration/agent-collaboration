//! Feature-gated entry and admission observation for real Host startup contract tests.
use super::{LocalDelivery, MailDeliveryRuntime, RouterDeliveryTypes};
use collaboration_mail_protocol::WakeBody;
use fleet_sync_protocol::DeliveryItem;

impl MailDeliveryRuntime {
    /// Drives the actual attached inbound consumer without replacing its wake owner.
    pub async fn test_support_deliver_inbound_wake(
        &self,
        item: &DeliveryItem<RouterDeliveryTypes>,
        body: &WakeBody,
    ) -> LocalDelivery<RouterDeliveryTypes> {
        self.consumer.deliver_wake(item, body).await
    }

    /// Observes admission so tests can wait causally before exercising another consumer.
    #[must_use]
    pub fn test_support_available_wake_permits(&self) -> Option<usize> {
        self.consumer
            .wakes
            .get()
            .map(|owner| owner.permits.available_permits())
    }
}
