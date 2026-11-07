//! Exhaustive mappings keep new native client failures visible at the Host boundary.
use board_sync_protocol::FleetLinkFailureReason;
use fleet_sync_session::{ClientError, ConnectFailure, ConnectionClosed, StoreError};
use runtime_primitives::WakeFailure;

pub(super) fn connect_failure(error: &ConnectFailure) -> FleetLinkFailureReason {
    match error {
        ConnectFailure::Unavailable => FleetLinkFailureReason::Unavailable,
        ConnectFailure::InvalidEndpoint => FleetLinkFailureReason::InvalidEndpoint,
        ConnectFailure::InvalidCredential => FleetLinkFailureReason::InvalidCredential,
        ConnectFailure::CredentialRejected(reason) => {
            FleetLinkFailureReason::CredentialRejected { reason: *reason }
        }
    }
}

pub(super) fn client_failure(error: &ClientError) -> FleetLinkFailureReason {
    match error {
        ClientError::Connect(error) => connect_failure(error),
        ClientError::Connection(ConnectionClosed::Transport) => {
            FleetLinkFailureReason::TransportError
        }
        ClientError::Connection(ConnectionClosed::UnsupportedFrame) => {
            FleetLinkFailureReason::UnsupportedFrame
        }
        ClientError::Connection(ConnectionClosed::IdleTimeout) => {
            FleetLinkFailureReason::IdleTimeout
        }
        ClientError::Connection(ConnectionClosed::SendTimeout) => {
            FleetLinkFailureReason::SendTimeout
        }
        ClientError::Store(StoreError::Unavailable { message }) => {
            FleetLinkFailureReason::StoreUnavailable {
                message: message.clone(),
            }
        }
        ClientError::Store(StoreError::Corrupt { field }) => FleetLinkFailureReason::CorruptStore {
            field: field.clone(),
        },
        ClientError::Wake(WakeFailure::Unavailable { message }) => {
            FleetLinkFailureReason::WakeUnavailable {
                message: message.clone(),
            }
        }
        ClientError::Wake(WakeFailure::InvalidDeadline { at }) => {
            FleetLinkFailureReason::InvalidWakeDeadline {
                at_unix_ms: at.unix_millis(),
            }
        }
        ClientError::Encoding => FleetLinkFailureReason::Encoding,
        ClientError::Protocol => FleetLinkFailureReason::Protocol,
        ClientError::NotConnected => FleetLinkFailureReason::NotConnected,
        ClientError::FrameInFlight => FleetLinkFailureReason::FrameInFlight,
        ClientError::RetryAt { at } => FleetLinkFailureReason::RetryAt {
            at_unix_ms: at.unix_millis(),
        },
        ClientError::SessionEnded => FleetLinkFailureReason::SessionEnded,
        ClientError::Revoked => FleetLinkFailureReason::CredentialRejected {
            reason: fleet_sync_protocol::AuthRefusal::Revoked,
        },
        ClientError::CredentialRejected { reason } => {
            FleetLinkFailureReason::CredentialRejected { reason: *reason }
        }
        ClientError::ReplacedByNewerSession => FleetLinkFailureReason::ReplacedByNewerSession,
        ClientError::StaleNoticeAcknowledgement { expected, received } => {
            FleetLinkFailureReason::StaleNoticeAcknowledgement {
                expected: *expected,
                received: *received,
            }
        }
    }
}

#[cfg(test)]
#[path = "fleet_link_errors_test.rs"]
mod tests;
