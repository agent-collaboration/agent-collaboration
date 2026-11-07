//! Diagnostic outcomes retain native refusal/close payloads without credentials.
use chrono::{DateTime, Utc};
use fleet_protocol::{AuthRefusal, CloseReason};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetLinkFailure {
    pub reason: FleetLinkFailureReason,
    pub at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetLinkFailureReason {
    Unavailable,
    InvalidEndpoint,
    InvalidCredential,
    CredentialRejected {
        #[schemars(schema_with = "auth_refusal_schema")]
        reason: AuthRefusal,
    },
    WelcomeTimeout,
    IdleTimeout,
    TransportError,
    SendTimeout,
    UnsupportedFrame,
    SessionClosed {
        #[schemars(schema_with = "close_reason_schema")]
        reason: CloseReason,
    },
    WebSocketClosed {
        code: u16,
        reason: String,
    },
    StoreUnavailable {
        message: String,
    },
    CorruptStore {
        field: String,
    },
    BoardPreparation {
        message: String,
    },
    WakeUnavailable {
        message: String,
    },
    InvalidWakeDeadline {
        at_unix_ms: i64,
    },
    Encoding,
    Protocol,
    NotConnected,
    FrameInFlight,
    RetryAt {
        at_unix_ms: i64,
    },
    SessionEnded,
    SessionStopped,
    ReplacedByNewerSession,
    StaleNoticeAcknowledgement {
        #[schemars(with = "message_board::RecordId")]
        expected: fleet_protocol::SessionId,
        #[schemars(with = "message_board::RecordId")]
        received: fleet_protocol::SessionId,
    },
}

impl FleetLinkFailureReason {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::InvalidEndpoint => "invalidEndpoint",
            Self::InvalidCredential => "invalidCredential",
            Self::CredentialRejected { .. } => "credentialRejected",
            Self::WelcomeTimeout => "welcomeTimeout",
            Self::IdleTimeout => "idleTimeout",
            Self::TransportError => "transportError",
            Self::SendTimeout => "sendTimeout",
            Self::UnsupportedFrame => "unsupportedFrame",
            Self::SessionClosed { .. } => "sessionClosed",
            Self::WebSocketClosed { .. } => "webSocketClosed",
            Self::StoreUnavailable { .. } => "storeUnavailable",
            Self::CorruptStore { .. } => "corruptStore",
            Self::BoardPreparation { .. } => "boardPreparation",
            Self::WakeUnavailable { .. } => "wakeUnavailable",
            Self::InvalidWakeDeadline { .. } => "invalidWakeDeadline",
            Self::Encoding => "encoding",
            Self::Protocol => "protocol",
            Self::NotConnected => "notConnected",
            Self::FrameInFlight => "frameInFlight",
            Self::RetryAt { .. } => "retryAt",
            Self::SessionEnded => "sessionEnded",
            Self::SessionStopped => "sessionStopped",
            Self::ReplacedByNewerSession => "replacedByNewerSession",
            Self::StaleNoticeAcknowledgement { .. } => "staleNoticeAcknowledgement",
        }
    }
}

// Native Serde types remain authoritative; upstream does not derive JsonSchema.
pub(crate) fn auth_refusal_schema(_: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({"type":"string","enum":["unknown","expired","revoked","wrongKind"]})
}
fn close_reason_schema(_: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({"oneOf":[
        {"type":"object","additionalProperties":false,"required":["kind"],"properties":{"kind":{"enum":["connectionReplaced","sessionEnded","malformedFrame","clientClosed"]}}},
        {"type":"object","additionalProperties":false,"required":["kind","value"],"properties":{"kind":{"const":"credentialRejected"},"value":{"type":"string","enum":["unknown","expired","revoked","wrongKind"]}}}
    ]})
}
