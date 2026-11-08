//! Socket handles and transport registrations, never session policy.

use collaboration_central_core::{
    AdmissionOrigin, NOTICE_PUSH_LIMIT, OWED_REQUEST_WINDOW, RegistrationEdits,
    RegistrationSnapshot, SocketFailure,
};
use fleet_protocol::{CloseReason, ConnectionId};
use fleet_session::{Bytes, ConnectionBinding, ConnectionRegistration};
use std::collections::BTreeMap;

/// Frames one connection's queue holds before its reader counts as stopped.
///
/// One event stages at most, for one connection, a window of owed requests
/// ([`OWED_REQUEST_WINDOW`], 32), one notice step ([`NOTICE_PUSH_LIMIT`], 32),
/// the answer to its own frame and a Welcome: 66 frames. Twice that leaves room
/// for a healthy reader still draining the previous event, so only a reader that
/// stopped reading overflows. Cloudflare's `WebSocket.send` buffers instead of
/// refusing, so overflow there is the platform's own behavior.
const SEND_QUEUE_FRAMES: usize = 2 * (OWED_REQUEST_WINDOW as usize + NOTICE_PUSH_LIMIT + 2);

/// One more slot, kept for the Close that tells a dropped reader why.
const SEND_QUEUE_CAPACITY: usize = SEND_QUEUE_FRAMES + 1;
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
        let (commands, receiver) = mpsc::channel(SEND_QUEUE_CAPACITY);
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

    /// Queue `command` without waiting: no event waits on one reader. A frame
    /// that finds the queue full refuses as `Closed`, and the caller drops that
    /// connection; its in-flight requests are owed again after it reconnects.
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
        // Frames leave the last slot to a Close.
        if matches!(&command, SocketCommand::Frame(_)) && sender.capacity() <= 1 {
            return Err(SocketFailure::Closed);
        }
        sender.try_send(command).map_err(|_| SocketFailure::Closed)
    }
}
