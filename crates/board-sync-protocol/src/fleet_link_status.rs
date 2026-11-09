//! The Router's observed central link, independent of enrollment validity.
use chrono::{DateTime, Utc};
use fleet_sync_protocol::PeerId;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetLinkRecord {
    #[schemars(schema_with = "peer_schema")]
    pub peer: PeerId,
    pub endpoint: String,
    pub instance: u64,
    pub connection: u64,
    pub revision: u64,
    pub last_frame_at: Option<DateTime<Utc>>,
    pub state: FleetLinkState,
    pub last_drop: Option<FleetLinkDrop>,
    pub last_failure: Option<crate::FleetLinkFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetLinkState {
    Disconnected {
        since: DateTime<Utc>,
    },
    Connecting {
        since: DateTime<Utc>,
        attempts: std::num::NonZeroU64,
    },
    Connected {
        since: DateTime<Utc>,
        last_contact: DateTime<Utc>,
    },
    Refused {
        reason: FleetLinkRefusal,
        at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FleetLinkRefusal {
    IdentityInUseElsewhere,
    CredentialRejected {
        #[schemars(schema_with = "crate::fleet_link_failure::auth_refusal_schema")]
        reason: fleet_sync_protocol::AuthRefusal,
    },
    SessionFailure {
        reason: crate::FleetLinkFailureReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetLinkDrop {
    pub reason: FleetLinkDropReason,
    pub at: DateTime<Utc>,
    pub connection: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FleetLinkDropReason {
    IdleTimeout,
    Closed,
    TransportError,
    SendTimeout,
    WelcomeTimeout,
    IdentityInUseElsewhere,
    ConnectFailed,
    UnsupportedFrame,
    CredentialRejected,
    Revoked,
}

impl FleetLinkDropReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdleTimeout => "idleTimeout",
            Self::Closed => "closed",
            Self::TransportError => "transportError",
            Self::SendTimeout => "sendTimeout",
            Self::WelcomeTimeout => "welcomeTimeout",
            Self::IdentityInUseElsewhere => "identityInUseElsewhere",
            Self::ConnectFailed => "connectFailed",
            Self::UnsupportedFrame => "unsupportedFrame",
            Self::CredentialRejected => "credentialRejected",
            Self::Revoked => "revoked",
        }
    }
}

impl FleetLinkRecord {
    pub fn is_valid(&self) -> bool {
        // Receipt precedes Welcome processing, so the native last-message time
        // may precede the instant when the link became ready.
        !self.endpoint.is_empty()
            && match &self.state {
                FleetLinkState::Connected { last_contact, .. } => {
                    self.last_frame_at == Some(*last_contact)
                }
                _ => true,
            }
    }
}

fn peer_schema(generator: &mut SchemaGenerator) -> Schema {
    message_board::RecordId::json_schema(generator)
}
