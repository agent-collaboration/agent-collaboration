//! Central delivering a direct message to the recipient's Router, and telling
//! the sender's Router how it ended (2c, Lead A design 2026-11-17). Each send
//! of a message carries a fresh `send_id`; the recipient's Ack names it, so a
//! late answer to an earlier send cannot settle a newer one.
use serde::{Deserialize, Serialize};

/// One send of a message to a Router: a UUIDv7 minted per send.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SendId(String);

impl SendId {
    /// Minted from the sender's injected clock and its UUIDv7 context.
    #[must_use]
    pub fn mint(timestamp: uuid::Timestamp) -> Self {
        Self(uuid::Uuid::new_v7(timestamp).to_string())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SendId {
    type Error = workspace_ids::IdValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let id: workspace_ids::RequestId = value.parse()?;
        Ok(Self(id.into()))
    }
}

impl From<SendId> for String {
    fn from(value: SendId) -> Self {
        value.0
    }
}

/// The recipient Router's answer to one Deliver. Every valid message goes to
/// the recipient's inbox (owner, record:297), so the only other answer is a
/// final refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryAnswer {
    Accepted,
    RefusedForGood,
}

pub use workspace_model::central_mail::{MessageCancellation, MessageOutcome, MessageRefusal};

#[cfg(test)]
#[path = "link_delivery_test.rs"]
mod tests;
