//! Mail borrows the Host's one prepared machine session.
use board_sync_protocol::BoardConsumerProtocol;
use durable_steps::StoreError;
use fleet_mail_delivery::{DeliveryTaskConfig, LocalDeliveryConfig};
use fleet_protocol::PeerId;
use fleet_session_client::SessionRequests;
use tokio::sync::watch;

#[derive(Clone)]
pub struct MailDeliverySession {
    pub requests: SessionRequests<BoardConsumerProtocol>,
    pub welcomed: watch::Receiver<bool>,
    pub peer: PeerId,
    pub relay: PeerId,
}

impl MailDeliverySession {
    pub fn task_config(
        &self,
        local: LocalDeliveryConfig,
    ) -> Result<DeliveryTaskConfig, StoreError> {
        Ok(DeliveryTaskConfig {
            local,
            relay: self.relay,
        })
    }
}
