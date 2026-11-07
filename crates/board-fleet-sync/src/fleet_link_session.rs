//! Present native link facts without synthesizing a second connection state.
use board_sync_protocol::{
    FleetLinkDrop, FleetLinkDropReason, FleetLinkFailure, FleetLinkFailureReason, FleetLinkRecord,
    FleetLinkRefusal, FleetLinkState,
};
use chrono::{DateTime, Utc};
use fleet_sync_protocol::{AuthRefusal, CloseReason};
use fleet_sync_session::StoreError;
use fleet_sync_tokio_runner::{
    SessionConnectFailureKind, SessionDropReason, SessionLinkEvent, SessionLinkEventKind,
    SessionLinkState, SessionLinkStatus,
};
use runtime_primitives::SyncTime;
use std::num::NonZeroU64;
use tokio::sync::broadcast;

pub(super) fn project_record(
    mut record: FleetLinkRecord,
    native: &SessionLinkState,
) -> Result<FleetLinkRecord, StoreError> {
    let since = timestamp(native.since, "sessionLink.since")?;
    record.instance = native.instance;
    record.connection = native.connection;
    record.revision = native.revision;
    if let Some(last_frame_at) = native.last_frame_at {
        record.last_frame_at = Some(timestamp(last_frame_at, "sessionLink.lastFrameAt")?);
    }
    record.state = match &native.status {
        SessionLinkStatus::Connecting { attempt } => FleetLinkState::Connecting {
            since,
            attempts: NonZeroU64::new(u64::from(*attempt))
                .ok_or_else(|| corrupt("sessionLink.attempt"))?,
        },
        SessionLinkStatus::Welcomed { .. } => FleetLinkState::Connected {
            since,
            last_contact: timestamp(
                native
                    .last_frame_at
                    .ok_or_else(|| corrupt("sessionLink.lastFrameAt"))?,
                "sessionLink.lastFrameAt",
            )?,
        },
        SessionLinkStatus::Refused { reason } => FleetLinkState::Refused {
            reason: refusal(reason),
            at: since,
        },
        SessionLinkStatus::CredentialRejected { reason, .. } => FleetLinkState::Refused {
            reason: FleetLinkRefusal::CredentialRejected { reason: *reason },
            at: since,
        },
        SessionLinkStatus::Revoked => FleetLinkState::Refused {
            reason: FleetLinkRefusal::CredentialRejected {
                reason: AuthRefusal::Revoked,
            },
            at: since,
        },
        SessionLinkStatus::Disconnected {
            reason: Some(CloseReason::ConnectionReplaced),
        } => FleetLinkState::Refused {
            reason: FleetLinkRefusal::IdentityInUseElsewhere,
            at: since,
        },
        SessionLinkStatus::Disconnected { .. } => FleetLinkState::Disconnected { since },
    };
    if let Some(dropped) = &native.last_drop {
        let at = timestamp(dropped.at, "sessionLink.lastDrop.at")?;
        record.last_drop = Some(FleetLinkDrop {
            reason: drop_reason(&dropped.reason),
            at,
            connection: dropped.connection,
        });
        record.last_failure = Some(FleetLinkFailure {
            reason: failure_reason(&dropped.reason),
            at,
        });
    }
    Ok(record)
}

fn refusal(reason: &SessionDropReason) -> FleetLinkRefusal {
    match reason {
        SessionDropReason::ReplacedByNewerSession
        | SessionDropReason::Disconnected {
            reason: Some(CloseReason::ConnectionReplaced),
        } => FleetLinkRefusal::IdentityInUseElsewhere,
        SessionDropReason::CredentialRejected { reason }
        | SessionDropReason::Disconnected {
            reason: Some(CloseReason::CredentialRejected(reason)),
        } => FleetLinkRefusal::CredentialRejected { reason: *reason },
        SessionDropReason::Revoked => FleetLinkRefusal::CredentialRejected {
            reason: AuthRefusal::Revoked,
        },
        _ => FleetLinkRefusal::SessionFailure {
            reason: failure_reason(reason),
        },
    }
}

fn failure_reason(reason: &SessionDropReason) -> FleetLinkFailureReason {
    match reason {
        SessionDropReason::ConnectFailed { kind } => match kind {
            SessionConnectFailureKind::Unavailable => FleetLinkFailureReason::Unavailable,
            SessionConnectFailureKind::InvalidEndpoint => FleetLinkFailureReason::InvalidEndpoint,
            SessionConnectFailureKind::InvalidCredential => {
                FleetLinkFailureReason::InvalidCredential
            }
        },
        SessionDropReason::UnsupportedFrame => FleetLinkFailureReason::UnsupportedFrame,
        SessionDropReason::ReplacedByNewerSession => FleetLinkFailureReason::ReplacedByNewerSession,
        SessionDropReason::IdleTimeout => FleetLinkFailureReason::IdleTimeout,
        SessionDropReason::SendTimeout => FleetLinkFailureReason::SendTimeout,
        SessionDropReason::Disconnected {
            reason: Some(reason),
        } => FleetLinkFailureReason::SessionClosed {
            reason: reason.clone(),
        },
        SessionDropReason::Disconnected { reason: None } => FleetLinkFailureReason::SessionStopped,
        SessionDropReason::CredentialRejected { reason } => {
            FleetLinkFailureReason::CredentialRejected { reason: *reason }
        }
        SessionDropReason::Revoked => FleetLinkFailureReason::CredentialRejected {
            reason: AuthRefusal::Revoked,
        },
        SessionDropReason::WelcomeTimeout => FleetLinkFailureReason::WelcomeTimeout,
    }
}

fn drop_reason(reason: &SessionDropReason) -> FleetLinkDropReason {
    match reason {
        SessionDropReason::ConnectFailed { .. } => FleetLinkDropReason::ConnectFailed,
        SessionDropReason::UnsupportedFrame => FleetLinkDropReason::UnsupportedFrame,
        SessionDropReason::ReplacedByNewerSession
        | SessionDropReason::Disconnected {
            reason: Some(CloseReason::ConnectionReplaced),
        } => FleetLinkDropReason::IdentityInUseElsewhere,
        SessionDropReason::IdleTimeout => FleetLinkDropReason::IdleTimeout,
        SessionDropReason::SendTimeout => FleetLinkDropReason::SendTimeout,
        SessionDropReason::Disconnected { .. } => FleetLinkDropReason::Closed,
        SessionDropReason::CredentialRejected { .. } => FleetLinkDropReason::CredentialRejected,
        SessionDropReason::Revoked => FleetLinkDropReason::Revoked,
        SessionDropReason::WelcomeTimeout => FleetLinkDropReason::WelcomeTimeout,
    }
}

fn timestamp(at: SyncTime, field: &'static str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::from_timestamp_millis(at.unix_millis()).ok_or_else(|| corrupt(field))
}
fn corrupt(field: &'static str) -> StoreError {
    StoreError::Corrupt {
        field: field.to_owned(),
    }
}

pub(super) async fn observe_events(mut events: broadcast::Receiver<SessionLinkEvent>) {
    loop {
        match events.recv().await {
            Ok(event) => log_event(event),
            Err(broadcast::error::RecvError::Lagged(skipped)) => tracing::warn!(
                event = "fleet.link.observerLagged",
                skipped,
                "fleet.link.observerLagged"
            ),
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

fn log_event(event: SessionLinkEvent) {
    let connection = event.connection;
    match event.kind {
        SessionLinkEventKind::Refused { reason } => {
            tracing::warn!(event = "fleet.link.refused", connection, reason = ?reason, "fleet.link.refused")
        }
        SessionLinkEventKind::Connecting { attempt } => tracing::info!(
            event = "fleet.link.connecting",
            connection,
            attempt,
            "fleet.link.connecting"
        ),
        SessionLinkEventKind::Welcomed { session_id } => {
            tracing::info!(event = "fleet.link.connected", connection, %session_id, "fleet.link.connected")
        }
        SessionLinkEventKind::Disconnected { reason } => {
            tracing::warn!(event = "fleet.link.disconnected", connection, reason = ?reason, "fleet.link.disconnected")
        }
        SessionLinkEventKind::CredentialRejected { reason } => tracing::warn!(
            event = "fleet.link.credentialRejected",
            connection,
            ?reason,
            "fleet.link.credentialRejected"
        ),
        SessionLinkEventKind::SessionRestarted { old, new } => {
            tracing::info!(event = "fleet.link.sessionRestarted", connection, %old, %new, "fleet.link.sessionRestarted")
        }
        SessionLinkEventKind::Revoked => tracing::error!(
            event = "fleet.link.revoked",
            connection,
            "fleet.link.revoked"
        ),
        SessionLinkEventKind::WelcomeTimeout => tracing::warn!(
            event = "fleet.link.welcomeTimeout",
            connection,
            "fleet.link.welcomeTimeout"
        ),
        SessionLinkEventKind::ReplacedByNewerSession => tracing::warn!(
            event = "fleet.link.replacedByNewerSession",
            connection,
            "fleet.link.replacedByNewerSession"
        ),
    }
}
