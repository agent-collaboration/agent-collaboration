//! The existing Fleet pump retains connection ownership; each connect reads the latest token.
use std::sync::Arc;

use board_fleet_sync::{ReqwestSessionConnection, ReqwestSessionTransport};
use codex_router_core::redaction::SecretString;
use fleet_identity_protocol::EnrollmentId;
use fleet_sync_protocol::CloseReason;
use fleet_sync_session::{
    Bytes, ConnectFailure, ConnectionClosed, Endpoint, PeerCredential, SessionTransport,
    TransportConnection,
};
use runtime_primitives::SyncTime;
use tokio::sync::watch;

use super::login_clock::FleetLoginClock;

#[derive(Clone, Debug)]
pub(super) enum MachineAccessToken {
    NotLoggedIn,
    AwaitingRoute {
        enrollment_id: EnrollmentId,
        token: SecretString,
        expires_at: SyncTime,
    },
    Granted {
        enrollment_id: EnrollmentId,
        token: SecretString,
        expires_at: SyncTime,
    },
}

struct MachineConnectionGrant {
    enrollment_id: EnrollmentId,
    credential: PeerCredential,
}

#[derive(Clone)]
pub(crate) struct MachineTokenSource {
    pub(super) token: watch::Receiver<MachineAccessToken>,
    clock: Arc<dyn FleetLoginClock>,
}

impl collaboration_contracts::MachineLoginGate for MachineTokenSource {
    fn machine_login_state(&self) -> collaboration_contracts::MachineLoginState {
        if self.is_current() {
            collaboration_contracts::MachineLoginState::LoggedIn
        } else {
            collaboration_contracts::MachineLoginState::NotLoggedIn
        }
    }
}

impl MachineTokenSource {
    pub(super) fn channel(
        clock: Arc<dyn FleetLoginClock>,
    ) -> (watch::Sender<MachineAccessToken>, Self) {
        let (sender, token) = watch::channel(MachineAccessToken::NotLoggedIn);
        (sender, Self { token, clock })
    }

    pub(super) fn is_current(&self) -> bool {
        if self.token.has_changed().is_err() {
            return false;
        }
        match &*self.token.borrow() {
            MachineAccessToken::Granted { expires_at, .. } => *expires_at > self.clock.now(),
            MachineAccessToken::NotLoggedIn | MachineAccessToken::AwaitingRoute { .. } => false,
        }
    }

    fn is_current_for(&self, enrollment: &EnrollmentId) -> bool {
        if self.token.has_changed().is_err() {
            return false;
        }
        matches!(&*self.token.borrow(), MachineAccessToken::Granted { enrollment_id, expires_at, .. }
            if enrollment_id == enrollment && *expires_at > self.clock.now())
    }

    async fn receive_for(
        &self,
        enrollment: &EnrollmentId,
        connection: &mut impl TransportConnection,
    ) -> Option<Result<Bytes, ConnectionClosed>> {
        let mut changes = self.token.clone();
        loop {
            if changes.has_changed().is_err() {
                return Some(Err(ConnectionClosed::Transport));
            }
            let expires_at = match &*changes.borrow_and_update() {
                MachineAccessToken::Granted {
                    enrollment_id,
                    expires_at,
                    ..
                } if enrollment_id == enrollment && *expires_at > self.clock.now() => *expires_at,
                _ => return Some(Err(ConnectionClosed::Transport)),
            };
            tokio::select! {
                biased;
                changed = changes.changed() => if changed.is_err() { return Some(Err(ConnectionClosed::Transport)); },
                _ = self.clock.wait_until(expires_at) => {},
                frame = connection.receive() => {
                    return if self.is_current_for(enrollment) { frame } else { Some(Err(ConnectionClosed::Transport)) };
                },
            }
        }
    }

    async fn next_credential(&self) -> Result<MachineConnectionGrant, ConnectFailure> {
        let mut token = self.token.clone();
        loop {
            if token.has_changed().is_err() {
                return Err(ConnectFailure::Unavailable);
            }
            let snapshot = token.borrow().clone();
            match snapshot {
                MachineAccessToken::Granted {
                    token,
                    expires_at,
                    enrollment_id,
                } if expires_at > self.clock.now() => {
                    return Ok(MachineConnectionGrant {
                        enrollment_id,
                        credential: PeerCredential::new(format!(
                            "Bearer {}",
                            token.expose_secret()
                        ))?,
                    });
                }
                MachineAccessToken::Granted { .. }
                | MachineAccessToken::NotLoggedIn
                | MachineAccessToken::AwaitingRoute { .. } => {}
            }
            token
                .changed()
                .await
                .map_err(|_| ConnectFailure::Unavailable)?;
        }
    }
}

#[derive(Clone)]
pub(crate) enum HostMachineSessionTransport {
    Legacy(ReqwestSessionTransport),
    DeviceCode {
        transport: ReqwestSessionTransport,
        source: MachineTokenSource,
    },
}

pub(crate) enum HostMachineSessionConnection {
    Legacy(ReqwestSessionConnection),
    DeviceCode {
        connection: ReqwestSessionConnection,
        source: MachineTokenSource,
        enrollment_id: EnrollmentId,
    },
}

impl SessionTransport for HostMachineSessionTransport {
    type Connection = HostMachineSessionConnection;
    async fn connect(
        &self,
        endpoint: &Endpoint,
        credential: &PeerCredential,
    ) -> Result<Self::Connection, ConnectFailure> {
        match self {
            Self::Legacy(transport) => transport
                .connect(endpoint, credential)
                .await
                .map(HostMachineSessionConnection::Legacy),
            Self::DeviceCode { transport, source } => {
                let current = source.next_credential().await?;
                let connection = transport.connect(endpoint, &current.credential).await?;
                if !source.is_current_for(&current.enrollment_id) {
                    connection.close(CloseReason::ConnectionReplaced).await;
                    return Err(ConnectFailure::Unavailable);
                }
                Ok(HostMachineSessionConnection::DeviceCode {
                    connection,
                    source: source.clone(),
                    enrollment_id: current.enrollment_id,
                })
            }
        }
    }
}

impl TransportConnection for HostMachineSessionConnection {
    async fn send(&mut self, frame: Bytes) -> Result<(), ConnectionClosed> {
        match self {
            Self::Legacy(connection) => connection.send(frame).await,
            Self::DeviceCode {
                connection,
                source,
                enrollment_id,
            } => {
                if !source.is_current_for(enrollment_id) {
                    return Err(ConnectionClosed::Transport);
                }
                connection.send(frame).await
            }
        }
    }
    async fn receive(&mut self) -> Option<Result<Bytes, ConnectionClosed>> {
        match self {
            Self::Legacy(connection) => connection.receive().await,
            Self::DeviceCode {
                connection,
                source,
                enrollment_id,
            } => source.receive_for(enrollment_id, connection).await,
        }
    }
    async fn close(self, reason: CloseReason) {
        match self {
            Self::Legacy(connection) | Self::DeviceCode { connection, .. } => {
                connection.close(reason).await
            }
        }
    }
}

#[cfg(test)]
#[path = "machine_token_transport_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "machine_token_transport_http_tests.rs"]
mod http_tests;
