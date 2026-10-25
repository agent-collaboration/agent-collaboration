//! Addresses and time at the shared application boundary.
use chrono::{DateTime, Utc};
use fleet_sync_protocol::DatabaseId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ObjectCallId(String);
impl ObjectCallId {
    pub fn new(value: impl Into<String>) -> Result<Self, runtime_primitives::InvalidOperationId> {
        Self::try_from(value.into())
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for ObjectCallId {
    type Error = runtime_primitives::InvalidOperationId;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        durable_steps::OperationId::try_from(value.clone())?;
        Ok(Self(value))
    }
}
impl From<ObjectCallId> for String {
    fn from(value: ObjectCallId) -> Self {
        value.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RegistryTime(pub DateTime<Utc>);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum ObjectAddress {
    Copy(DatabaseId),
    Registry,
    Fleet,
    Connection(message_board::ServiceId),
    Mailbox(message_board::ServiceId),
}
impl ObjectAddress {
    #[must_use]
    pub const fn primary_face(&self) -> crate::CallerKind {
        match self {
            Self::Connection(_) => crate::CallerKind::Session,
            Self::Registry => crate::CallerKind::Registry,
            Self::Copy(_) => crate::CallerKind::Copy,
            Self::Fleet => crate::CallerKind::Fleet,
            Self::Mailbox(_) => crate::CallerKind::Mailbox,
        }
    }
}
