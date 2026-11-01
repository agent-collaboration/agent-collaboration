//! The existing Fleet pump retains connection ownership; each connect reads the latest token.
use std::sync::Arc;

use board_fleet_sync::{ReqwestSessionConnection, ReqwestSessionTransport};
use codex_router_core::redaction::SecretString;
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
    Granted {
        token: SecretString,
        expires_at: SyncTime,
    },
}

#[derive(Clone)]
pub(crate) struct MachineTokenSource {
    token: watch::Receiver<MachineAccessToken>,
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
        match &*self.token.borrow() {
            MachineAccessToken::Granted { expires_at, .. } => *expires_at > self.clock.now(),
            MachineAccessToken::NotLoggedIn => false,
        }
    }

    async fn next_credential(&self) -> Result<PeerCredential, ConnectFailure> {
        let mut token = self.token.clone();
        loop {
            let snapshot = token.borrow().clone();
            match snapshot {
                MachineAccessToken::Granted { token, expires_at }
                    if expires_at > self.clock.now() =>
                {
                    return PeerCredential::new(format!("Bearer {}", token.expose_secret()));
                }
                MachineAccessToken::Granted { .. } | MachineAccessToken::NotLoggedIn => {}
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
                let connection = transport.connect(endpoint, &current).await?;
                Ok(HostMachineSessionConnection::DeviceCode {
                    connection,
                    source: source.clone(),
                })
            }
        }
    }
}

impl TransportConnection for HostMachineSessionConnection {
    async fn send(&mut self, frame: Bytes) -> Result<(), ConnectionClosed> {
        match self {
            Self::Legacy(connection) => connection.send(frame).await,
            Self::DeviceCode { connection, source } => {
                if !source.is_current() {
                    return Err(ConnectionClosed::Transport);
                }
                connection.send(frame).await
            }
        }
    }
    async fn receive(&mut self) -> Option<Result<Bytes, ConnectionClosed>> {
        match self {
            Self::Legacy(connection) => connection.receive().await,
            Self::DeviceCode { connection, source } => {
                if !source.is_current() {
                    return Some(Err(ConnectionClosed::Transport));
                }
                connection.receive().await
            }
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
