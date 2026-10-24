//! Central's delivery quotas include payload and reserved settlement bytes.

use board_sync_protocol::BoardDeliveryTypes;
use fleet_sync_delivery::{DeliveryConfigError, DeliveryLimits, minimum_settlement_allowance};

pub fn central_mailbox_limits() -> Result<DeliveryLimits, DeliveryConfigError> {
    DeliveryLimits::new::<BoardDeliveryTypes>(
        1024 * 1024 * 1024,
        200_000,
        256 * 1024 * 1024,
        50_000,
        256 * 1024 * 1024,
        minimum_settlement_allowance::<BoardDeliveryTypes>()?,
    )
}

/// Keep production byte reservations while making item-cap exhaustion practical in conformance.
#[cfg(feature = "conformance")]
pub fn conformance_mailbox_item_limit(
    items: std::num::NonZeroU64,
) -> Result<DeliveryLimits, DeliveryConfigError> {
    let limits = central_mailbox_limits()?;
    DeliveryLimits::new::<BoardDeliveryTypes>(
        limits.mailbox_payload_bytes(),
        items.get(),
        limits.outbox_bytes(),
        limits.outbox_items(),
        limits.inbox_bytes(),
        limits.settlement_allowance(),
    )
}
