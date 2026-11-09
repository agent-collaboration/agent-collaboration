//! Readable typed diagnostic tags and details, never JSON payload storage.
use super::super::{RouterLocalStorageError, invalid};
use board_sync_protocol::{
    FleetLinkDropReason as DropReason, FleetLinkFailureReason as Failure,
    FleetLinkRefusal as Refusal,
};
use fleet_protocol::{AuthRefusal, CloseReason, SessionId};

pub(super) fn encode_refusal(reason: &Refusal) -> (&'static str, Option<String>) {
    match reason {
        Refusal::IdentityInUseElsewhere => ("identityInUseElsewhere", None),
        Refusal::CredentialRejected { reason } => {
            ("credentialRejected", Some(auth_tag(*reason).to_owned()))
        }
        Refusal::SessionFailure { reason } => {
            let (kind, detail) = encode_failure(reason);
            (
                "sessionFailure",
                Some(match detail {
                    Some(detail) => format!("{kind}: {detail}"),
                    None => kind.to_owned(),
                }),
            )
        }
    }
}
pub(super) fn decode_refusal(
    kind: &str,
    detail: Option<&str>,
) -> Result<Refusal, RouterLocalStorageError> {
    match (kind, detail) {
        ("identityInUseElsewhere", None) => Ok(Refusal::IdentityInUseElsewhere),
        ("credentialRejected", Some(detail)) => Ok(Refusal::CredentialRejected {
            reason: decode_auth(detail)?,
        }),
        ("sessionFailure", Some(detail)) => {
            let (kind, detail) = detail
                .split_once(": ")
                .map_or((detail, None), |(kind, detail)| (kind, Some(detail)));
            Ok(Refusal::SessionFailure {
                reason: decode_failure(kind, detail)?,
            })
        }
        _ => Err(invalid("fleetLink.refusal")),
    }
}
pub(super) fn encode_failure(reason: &Failure) -> (&'static str, Option<String>) {
    let detail = match reason {
        Failure::CredentialRejected { reason } => Some(auth_tag(*reason).to_owned()),
        Failure::SessionClosed { reason } => Some(close_detail(reason)),
        Failure::WebSocketClosed { code, reason } => Some(format!("{code}: {reason}")),
        Failure::StoreUnavailable { message }
        | Failure::BoardPreparation { message }
        | Failure::WakeUnavailable { message } => Some(message.clone()),
        Failure::CorruptStore { field } => Some(field.clone()),
        Failure::InvalidWakeDeadline { at_unix_ms } | Failure::RetryAt { at_unix_ms } => {
            Some(at_unix_ms.to_string())
        }
        Failure::StaleNoticeAcknowledgement { expected, received } => {
            Some(format!("{expected} {received}"))
        }
        Failure::Unavailable
        | Failure::InvalidEndpoint
        | Failure::InvalidCredential
        | Failure::WelcomeTimeout
        | Failure::IdleTimeout
        | Failure::TransportError
        | Failure::SendTimeout
        | Failure::UnsupportedFrame
        | Failure::Encoding
        | Failure::Protocol
        | Failure::NotConnected
        | Failure::FrameInFlight
        | Failure::SessionEnded
        | Failure::SessionStopped
        | Failure::ReplacedByNewerSession => None,
    };
    (reason.as_str(), detail)
}
pub(super) fn decode_failure(
    kind: &str,
    detail: Option<&str>,
) -> Result<Failure, RouterLocalStorageError> {
    Ok(match (kind, detail) {
        ("unavailable", None) => Failure::Unavailable,
        ("invalidEndpoint", None) => Failure::InvalidEndpoint,
        ("invalidCredential", None) => Failure::InvalidCredential,
        ("welcomeTimeout", None) => Failure::WelcomeTimeout,
        ("idleTimeout", None) => Failure::IdleTimeout,
        ("transportError", None) => Failure::TransportError,
        ("sendTimeout", None) => Failure::SendTimeout,
        ("unsupportedFrame", None) => Failure::UnsupportedFrame,
        ("encoding", None) => Failure::Encoding,
        ("protocol", None) => Failure::Protocol,
        ("notConnected", None) => Failure::NotConnected,
        ("frameInFlight", None) => Failure::FrameInFlight,
        ("sessionEnded", None) => Failure::SessionEnded,
        ("sessionStopped", None) => Failure::SessionStopped,
        ("replacedByNewerSession", None) => Failure::ReplacedByNewerSession,
        ("credentialRejected", Some(detail)) => Failure::CredentialRejected {
            reason: decode_auth(detail)?,
        },
        ("sessionClosed", Some(detail)) => Failure::SessionClosed {
            reason: decode_close(detail)?,
        },
        ("webSocketClosed", Some(detail)) => {
            let (code, reason) = detail
                .split_once(": ")
                .ok_or_else(|| invalid("fleetLink.failureDetail"))?;
            let value = code
                .parse::<u16>()
                .map_err(|_| invalid("fleetLink.failureDetail"))?;
            if value.to_string() != code {
                return Err(invalid("fleetLink.failureDetail"));
            }
            Failure::WebSocketClosed {
                code: value,
                reason: reason.to_owned(),
            }
        }
        ("storeUnavailable", Some(detail)) => Failure::StoreUnavailable {
            message: detail.to_owned(),
        },
        ("boardPreparation", Some(detail)) => Failure::BoardPreparation {
            message: detail.to_owned(),
        },
        ("wakeUnavailable", Some(detail)) => Failure::WakeUnavailable {
            message: detail.to_owned(),
        },
        ("corruptStore", Some(detail)) => Failure::CorruptStore {
            field: detail.to_owned(),
        },
        ("invalidWakeDeadline" | "retryAt", Some(detail)) => {
            let at_unix_ms = detail
                .parse::<i64>()
                .map_err(|_| invalid("fleetLink.failureDetail"))?;
            if at_unix_ms.to_string() != detail {
                return Err(invalid("fleetLink.failureDetail"));
            }
            if kind == "retryAt" {
                Failure::RetryAt { at_unix_ms }
            } else {
                Failure::InvalidWakeDeadline { at_unix_ms }
            }
        }
        ("staleNoticeAcknowledgement", Some(detail)) => {
            let (expected, received) = detail
                .split_once(' ')
                .ok_or_else(|| invalid("fleetLink.failureDetail"))?;
            Failure::StaleNoticeAcknowledgement {
                expected: session_id(expected)?,
                received: session_id(received)?,
            }
        }
        _ => return Err(invalid("fleetLink.failure")),
    })
}
fn session_id(text: &str) -> Result<SessionId, RouterLocalStorageError> {
    super::super::validate_id(text, "fleetLink.failureDetail")?;
    SessionId::new(uuid::Uuid::parse_str(text).map_err(|_| invalid("fleetLink.failureDetail"))?)
        .map_err(|_| invalid("fleetLink.failureDetail"))
}
fn auth_tag(reason: AuthRefusal) -> &'static str {
    match reason {
        AuthRefusal::Unknown => "unknown",
        AuthRefusal::Expired => "expired",
        AuthRefusal::Revoked => "revoked",
        AuthRefusal::WrongKind => "wrongKind",
    }
}
fn decode_auth(detail: &str) -> Result<AuthRefusal, RouterLocalStorageError> {
    match detail {
        "unknown" => Ok(AuthRefusal::Unknown),
        "expired" => Ok(AuthRefusal::Expired),
        "revoked" => Ok(AuthRefusal::Revoked),
        "wrongKind" => Ok(AuthRefusal::WrongKind),
        _ => Err(invalid("fleetLink.authRefusal")),
    }
}
fn close_detail(reason: &CloseReason) -> String {
    match reason {
        CloseReason::ConnectionReplaced => "connectionReplaced".to_owned(),
        CloseReason::SessionEnded => "sessionEnded".to_owned(),
        CloseReason::MalformedFrame => "malformedFrame".to_owned(),
        CloseReason::ClientClosed => "clientClosed".to_owned(),
        CloseReason::CredentialRejected(reason) => {
            format!("credentialRejected: {}", auth_tag(*reason))
        }
    }
}
fn decode_close(detail: &str) -> Result<CloseReason, RouterLocalStorageError> {
    match detail {
        "connectionReplaced" => Ok(CloseReason::ConnectionReplaced),
        "sessionEnded" => Ok(CloseReason::SessionEnded),
        "malformedFrame" => Ok(CloseReason::MalformedFrame),
        "clientClosed" => Ok(CloseReason::ClientClosed),
        text if text.starts_with("credentialRejected: ") => {
            Ok(CloseReason::CredentialRejected(decode_auth(
                text.strip_prefix("credentialRejected: ")
                    .ok_or_else(|| invalid("fleetLink.closeReason"))?,
            )?))
        }
        _ => Err(invalid("fleetLink.closeReason")),
    }
}
pub(super) fn decode_drop(kind: &str) -> Result<DropReason, RouterLocalStorageError> {
    match kind {
        "idleTimeout" => Ok(DropReason::IdleTimeout),
        "closed" => Ok(DropReason::Closed),
        "transportError" => Ok(DropReason::TransportError),
        "sendTimeout" => Ok(DropReason::SendTimeout),
        "welcomeTimeout" => Ok(DropReason::WelcomeTimeout),
        "identityInUseElsewhere" => Ok(DropReason::IdentityInUseElsewhere),
        "connectFailed" => Ok(DropReason::ConnectFailed),
        "unsupportedFrame" => Ok(DropReason::UnsupportedFrame),
        "credentialRejected" => Ok(DropReason::CredentialRejected),
        "revoked" => Ok(DropReason::Revoked),
        _ => Err(invalid("fleetLink.lastDropKind")),
    }
}
