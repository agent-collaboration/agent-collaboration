use super::*;
use fleet_protocol::AuthRefusal;

#[test]
fn native_client_failure_codes_and_payloads_are_retained_exhaustively() {
    use FleetLinkFailureReason as Failure;
    let expected = fleet_protocol::SessionId::new(uuid::Uuid::now_v7()).unwrap();
    let received = fleet_protocol::SessionId::new(uuid::Uuid::now_v7()).unwrap();
    let cases = [
        (
            ClientError::Revoked,
            Failure::CredentialRejected {
                reason: AuthRefusal::Revoked,
            },
        ),
        (
            ClientError::CredentialRejected {
                reason: AuthRefusal::WrongKind,
            },
            Failure::CredentialRejected {
                reason: AuthRefusal::WrongKind,
            },
        ),
        (
            ClientError::ReplacedByNewerSession,
            Failure::ReplacedByNewerSession,
        ),
        (
            ClientError::ConnectionReplaced,
            Failure::SessionClosed {
                reason: fleet_protocol::CloseReason::ConnectionReplaced,
            },
        ),
        (
            ClientError::StaleNoticeAcknowledgement { expected, received },
            Failure::StaleNoticeAcknowledgement { expected, received },
        ),
        (
            ClientError::Connect(ConnectFailure::Unavailable),
            Failure::Unavailable,
        ),
        (
            ClientError::Connect(ConnectFailure::InvalidEndpoint),
            Failure::InvalidEndpoint,
        ),
        (
            ClientError::Connect(ConnectFailure::InvalidCredential),
            Failure::InvalidCredential,
        ),
        (
            ClientError::Connect(ConnectFailure::CredentialRejected(AuthRefusal::Expired)),
            Failure::CredentialRejected {
                reason: AuthRefusal::Expired,
            },
        ),
        (
            ClientError::Connection(ConnectionClosed::Transport),
            Failure::TransportError,
        ),
        (
            ClientError::Connection(ConnectionClosed::UnsupportedFrame),
            Failure::UnsupportedFrame,
        ),
        (
            ClientError::Connection(ConnectionClosed::IdleTimeout),
            Failure::IdleTimeout,
        ),
        (
            ClientError::Connection(ConnectionClosed::SendTimeout),
            Failure::SendTimeout,
        ),
        (
            ClientError::Store(StoreError::Corrupt {
                field: "machineSessionState".to_owned(),
            }),
            Failure::CorruptStore {
                field: "machineSessionState".to_owned(),
            },
        ),
        (
            ClientError::Store(StoreError::Unavailable {
                message: "fixture".to_owned(),
            }),
            Failure::StoreUnavailable {
                message: "fixture".to_owned(),
            },
        ),
        (
            ClientError::Wake(WakeFailure::Unavailable {
                message: "fixture".to_owned(),
            }),
            Failure::WakeUnavailable {
                message: "fixture".to_owned(),
            },
        ),
        (
            ClientError::Wake(WakeFailure::InvalidDeadline {
                at: runtime_primitives::SyncTime::from_unix_millis(17),
            }),
            Failure::InvalidWakeDeadline { at_unix_ms: 17 },
        ),
        (ClientError::Encoding, Failure::Encoding),
        (ClientError::Protocol, Failure::Protocol),
        (ClientError::NotConnected, Failure::NotConnected),
        (ClientError::FrameInFlight, Failure::FrameInFlight),
        (
            ClientError::RetryAt {
                at: runtime_primitives::SyncTime::from_unix_millis(19),
            },
            Failure::RetryAt { at_unix_ms: 19 },
        ),
        (ClientError::SessionEnded, Failure::SessionEnded),
    ];
    for (error, expected) in cases {
        assert_eq!(client_failure(&error), expected);
    }
}
