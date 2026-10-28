//! Stored native receipts remain readable while the public wire uses one new shape.
use collaboration_protocol::{DeliveryReceipt, NativeSendReceipt};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum StoredDeliveryReceipt {
    Current(DeliveryReceipt),
    LegacyNative(NativeSendReceipt),
}

impl StoredDeliveryReceipt {
    pub fn into_public(self) -> DeliveryReceipt {
        match self {
            Self::Current(receipt) => receipt,
            Self::LegacyNative(receipt) => receipt.into(),
        }
    }
}
