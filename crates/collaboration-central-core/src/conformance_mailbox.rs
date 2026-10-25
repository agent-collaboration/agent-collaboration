//! Fixed small quota for rule conformance, absent from production core builds.
use crate::{MailboxKind, ObjectHost, ObjectRuntime, PlatformIo, object_runtime::ObjectWork};
use board_sync_protocol::BoardDeliveryTypes;
use fleet_sync_delivery::{DeliveryConfigError, DeliveryLimits, minimum_settlement_allowance};

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<MailboxKind, Host, Io> {
    /// Select one payload slot while preserving the library's settlement reserve.
    pub fn conformance_small_mailbox_capacity(&self) -> Result<(), DeliveryConfigError> {
        if let ObjectWork::Mailbox {
            conformance_limits, ..
        } = &self.work
        {
            let limits = DeliveryLimits::new::<BoardDeliveryTypes>(
                2_000_000,
                1,
                2_000_000,
                100,
                2_000_000,
                minimum_settlement_allowance::<BoardDeliveryTypes>()?,
            )?;
            // Selecting the same fixed quota twice is harmless.
            let _ = conformance_limits.set(limits);
        }
        Ok(())
    }
    pub(crate) fn conformance_mailbox_limits(&self, default: DeliveryLimits) -> DeliveryLimits {
        match &self.work {
            ObjectWork::Mailbox {
                conformance_limits, ..
            } => conformance_limits.get().copied().unwrap_or(default),
            _ => default,
        }
    }
}
