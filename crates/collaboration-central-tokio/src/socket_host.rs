//! Socket handles and transport registrations, never session policy.

use collaboration_central_core::{
    AdmissionOrigin, RegistrationEdits, RegistrationSnapshot, SocketFailure,
};
use fleet_sync_protocol::{CloseReason, ConnectionId};
use fleet_sync_session::{Bytes, ConnectionBinding, ConnectionRegistration};
use std::collections::BTreeMap;
use tokio::sync::{Mutex, mpsc};

pub enum SocketCommand {
    Frame(Bytes),
    Close(CloseReason),
}

struct SocketState {
    binding: ConnectionBinding,
    admission_origin: AdmissionOrigin,
    registration: ConnectionRegistration,
    quarantined: bool,
    commands: mpsc::Sender<SocketCommand>,
}

#[derive(Default)]
pub(crate) struct TokioSockets(Mutex<BTreeMap<ConnectionId, SocketState>>);

impl TokioSockets {
    pub(crate) async fn attach(
        &self,
        binding: ConnectionBinding,
        admission_origin: AdmissionOrigin,
    ) -> mpsc::Receiver<SocketCommand> {
        let (commands, receiver) = mpsc::channel(64);
        self.0.lock().await.insert(
            binding.connection,
            SocketState {
                binding,
                admission_origin,
                registration: ConnectionRegistration::Opening,
                quarantined: false,
                commands,
            },
        );
        receiver
    }

    pub(crate) async fn admission_origin(
        &self,
        connection: ConnectionId,
    ) -> Result<AdmissionOrigin, SocketFailure> {
        let sockets = self.0.lock().await;
        let socket = sockets
            .get(&connection)
            .ok_or(SocketFailure::NotConnected)?;
        if socket.quarantined || socket.commands.is_closed() {
            return Err(SocketFailure::Closed);
        }
        Ok(socket.admission_origin.clone())
    }

    pub(crate) async fn admission_binding(
        &self,
        connection: ConnectionId,
    ) -> Result<ConnectionBinding, SocketFailure> {
        let sockets = self.0.lock().await;
        let socket = sockets
            .get(&connection)
            .ok_or(SocketFailure::NotConnected)?;
        if socket.quarantined || socket.commands.is_closed() {
            return Err(SocketFailure::Closed);
        }
        Ok(socket.binding)
    }

    pub(crate) async fn detach(&self, connection: ConnectionId) {
        self.0.lock().await.remove(&connection);
    }

    pub(crate) async fn registrations(&self) -> RegistrationSnapshot {
        self.0
            .lock()
            .await
            .iter()
            .filter(|(_, socket)| !socket.commands.is_closed())
            .map(|(connection, socket)| {
                (
                    *connection,
                    if socket.quarantined {
                        ConnectionRegistration::Closed
                    } else {
                        socket.registration
                    },
                )
            })
            .collect()
    }

    pub(crate) async fn publish(
        &self,
        edits: RegistrationEdits,
    ) -> Vec<(ConnectionId, SocketFailure)> {
        let mut sockets = self.0.lock().await;
        let mut failures = Vec::new();
        for (connection, registration) in edits {
            match sockets.get_mut(&connection) {
                Some(socket) if !socket.quarantined && !socket.commands.is_closed() => {
                    socket.registration = registration
                }
                Some(_) => failures.push((connection, SocketFailure::Closed)),
                None => failures.push((connection, SocketFailure::NotConnected)),
            }
        }
        failures
    }

    pub(crate) async fn quarantine(&self, connections: Vec<ConnectionId>) {
        let mut sockets = self.0.lock().await;
        for connection in connections {
            if let Some(socket) = sockets.get_mut(&connection) {
                socket.quarantined = true;
            }
        }
    }

    pub(crate) async fn send(
        &self,
        connection: ConnectionId,
        command: SocketCommand,
    ) -> Result<(), SocketFailure> {
        let sender = {
            let sockets = self.0.lock().await;
            let socket = sockets
                .get(&connection)
                .ok_or(SocketFailure::NotConnected)?;
            if socket.quarantined && matches!(&command, SocketCommand::Frame(_)) {
                return Err(SocketFailure::Closed);
            }
            socket.commands.clone()
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), sender.send(command))
            .await
            .map_err(|_| SocketFailure::Closed)?
            .map_err(|_| SocketFailure::Closed)
    }
}
