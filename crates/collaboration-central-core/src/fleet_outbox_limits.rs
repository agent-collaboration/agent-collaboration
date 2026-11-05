//! Central's delivery quotas include payload and reserved settlement bytes.

use board_sync_protocol::BoardDeliveryTypes;
use fleet_sync_delivery::{DeliveryConfigError, DeliveryLimits, minimum_settlement_allowance};

pub fn fleet_outbox_limits() -> Result<DeliveryLimits, DeliveryConfigError> {
    DeliveryLimits::new::<BoardDeliveryTypes>(
        256 * 1024 * 1024,
        50_000,
        256 * 1024 * 1024,
        minimum_settlement_allowance::<BoardDeliveryTypes>()?,
    )
}

#[cfg(test)]
#[path = "fleet_outbox_limits_test.rs"]
mod tests;
