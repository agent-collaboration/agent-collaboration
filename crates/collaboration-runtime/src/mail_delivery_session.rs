//! Mail borrows the Host's one prepared machine session.
use board_sync_protocol::BoardConsumerProtocol;
use durable_steps::StoreError;
use fleet_sync_delivery::{DeliveryTaskConfig, LocalDeliveryConfig};
use fleet_sync_protocol::{PeerId, unit_budget::TARGET_UNIT_BYTES};
use fleet_sync_tokio_runner::SessionRequests;
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
            fetch_items: 100,
            fetch_bytes: u64::try_from(TARGET_UNIT_BYTES).map_err(StoreError::backend)?,
        })
    }
}
