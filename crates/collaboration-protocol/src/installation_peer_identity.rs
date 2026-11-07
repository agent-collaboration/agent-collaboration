//! The installation's stable UUIDv7 peer is separate from its verbatim service UUID.
use message_board::RecordId;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallationIdentity {
    pub service_id: crate::UuidIdentity,
    pub machine_label: crate::MachineLabel,
    pub peer_id: InstallationPeerId,
    #[serde(default)]
    pub central: CentralLinkObservation,
}

/// Link diagnostics are independent of the installation's stable identity.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CentralLinkObservation {
    /// An older Host did not publish a link observation.
    #[default]
    NotObserved,
    NotConfigured,
    Available {
        record: Box<board_sync_protocol::FleetLinkRecord>,
    },
    Unavailable {
        message: String,
    },
}

impl CentralLinkObservation {
    pub fn record(&self) -> Option<&board_sync_protocol::FleetLinkRecord> {
        match self {
            Self::Available { record } => Some(record),
            Self::NotObserved | Self::NotConfigured | Self::Unavailable { .. } => None,
        }
    }

    pub fn into_record(self) -> Option<board_sync_protocol::FleetLinkRecord> {
        match self {
            Self::Available { record } => Some(*record),
            Self::NotObserved | Self::NotConfigured | Self::Unavailable { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct InstallationPeerId(RecordId);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("installation peer must be a canonical UUIDv7")]
pub struct InstallationPeerIdentityError;

impl TryFrom<String> for InstallationPeerId {
    type Error = InstallationPeerIdentityError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        RecordId::try_from(value)
            .map(Self)
            .map_err(|_| InstallationPeerIdentityError)
    }
}
impl From<InstallationPeerId> for String {
    fn from(peer: InstallationPeerId) -> Self {
        peer.0.into()
    }
}
impl InstallationPeerId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
impl JsonSchema for InstallationPeerId {
    fn schema_name() -> Cow<'static, str> {
        "InstallationPeerId".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        RecordId::json_schema(generator)
    }
}
