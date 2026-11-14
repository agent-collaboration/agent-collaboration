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
    Central(fleet_identity_protocol::OrganizationId),
    Copy(DatabaseId),
    Fleet,
    /// One workspace's store: it holds the tables and runs the statements it is given.
    WorkspaceStore(message_board::WorkspaceId),
}
impl ObjectAddress {
    /// The caller face an object call arrives under; a workspace store takes no object calls.
    #[must_use]
    pub const fn primary_face(&self) -> Option<crate::CallerKind> {
        match self {
            Self::Copy(_) => Some(crate::CallerKind::Copy),
            Self::Fleet => Some(crate::CallerKind::Fleet),
            Self::WorkspaceStore(_) => None,
            Self::Central(_) => None,
        }
    }
}
