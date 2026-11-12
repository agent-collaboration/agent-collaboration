//! Shared Durable Object store, one alarm owner and socket attachment I/O.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::arithmetic_side_effects,
    clippy::unimplemented,
    clippy::unreachable
)]

use crate::binding_attachment::BindingAttachment;
use collaboration_central_core::{
    AdmissionOrigin, LocalSectionFailure, ObjectHost, RegistrationEdits, RegistrationSnapshot,
    SocketFailure,
};
use durable_steps::{Host, durable_object_store::DurableObjectStore};
use fleet_sync_protocol::{CloseReason, ConnectionId};
use fleet_sync_session::{Bytes, ConnectionBinding, ConnectionRegistration};
use runtime_primitives::AlarmOwner;
use std::{
    collections::BTreeSet,
    future::{Future, ready},
    sync::{Arc, Mutex},
};
use worker::{
    State, WebSocket,
    send::{SendFuture, SendWrapper},
};

type SharedStore<SharedHost> = DurableObjectStore<SharedHost>;

type RuntimeStore<SharedHost> = SharedStore<SharedHost>;

pub struct CloudflareStorageHost<SharedHost: Host> {
    store: Arc<RuntimeStore<SharedHost>>,
    wake: AlarmOwner,
    state: Arc<SendWrapper<State>>,
    quarantine: Mutex<BTreeSet<ConnectionId>>,
}

impl<SharedHost: Host> CloudflareStorageHost<SharedHost> {
    /// Construct once for the object. The store, host and wake capability receive
    /// clones of this one alarm owner, never independent alarm arbiters.
    pub fn new(
        state: Arc<SendWrapper<State>>,
        shared_host: impl FnOnce(AlarmOwner) -> Arc<SharedHost>,
    ) -> Self {
        let wake = AlarmOwner::new(state.storage());
        let shared_host = shared_host(wake.clone());
        let store = DurableObjectStore::new(
            sqlx_durable_object::DoConnection::new(state.storage()),
            shared_host,
            wake.clone(),
        );
        let store = Arc::new(store);
        Self {
            store,
            wake,
            state,
            quarantine: Mutex::default(),
        }
    }

    pub async fn migrate_store(&self) -> Result<(), durable_steps::StoreError> {
        self.store.migrate().await
    }

    pub fn accept_socket(
        &self,
        socket: &WebSocket,
        binding: ConnectionBinding,
        admission_origin: AdmissionOrigin,
    ) -> Result<(), SocketFailure> {
        let attachment = BindingAttachment {
            connection: binding.connection,
            peer: binding.peer,
            expires_at: binding.expires_at,
            registration: ConnectionRegistration::Opening,
            admission_origin,
        };
        socket
            .serialize_attachment(attachment)
            .map_err(|_| SocketFailure::Closed)?;
        self.state.accept_web_socket(socket);
        Ok(())
    }

    pub fn binding(&self, socket: &WebSocket) -> Result<ConnectionBinding, SocketFailure> {
        Ok(Self::attachment(socket)?.binding())
    }

    pub fn socket_origin(&self, socket: &WebSocket) -> Result<AdmissionOrigin, SocketFailure> {
        Ok(Self::attachment(socket)?.admission_origin)
    }

    fn attachment(socket: &WebSocket) -> Result<BindingAttachment, SocketFailure> {
        socket
            .deserialize_attachment()
            .map_err(|_| SocketFailure::Closed)?
            .ok_or(SocketFailure::NotConnected)
    }
    fn socket(&self, connection: ConnectionId) -> Result<WebSocket, SocketFailure> {
        self.state
            .get_websockets()
            .into_iter()
            .find(|socket| {
                Self::attachment(socket).is_ok_and(|binding| binding.connection == connection)
            })
            .ok_or(SocketFailure::NotConnected)
    }
    fn is_open(socket: &WebSocket) -> bool {
        socket.as_ref().ready_state() == web_sys::WebSocket::OPEN
    }
    fn is_quarantined(&self, connection: ConnectionId) -> bool {
        self.quarantine
            .lock()
            .map_or(true, |quarantine| quarantine.contains(&connection))
    }
    fn publish(&self, edits: RegistrationEdits) -> Vec<(ConnectionId, SocketFailure)> {
        let mut failures = Vec::new();
        for (connection, registration) in edits {
            let write = || {
                let socket = self.socket(connection)?;
                if !Self::is_open(&socket) || self.is_quarantined(connection) {
                    return Err(SocketFailure::Closed);
                }
                let mut attachment = Self::attachment(&socket)?;
                attachment.registration = registration;
                socket
                    .serialize_attachment(attachment)
                    .map_err(|_| SocketFailure::Closed)
            };
            if let Err(failure) = write() {
                failures.push((connection, failure));
            }
        }
        failures
    }
}

impl<SharedHost: Host> ObjectHost for CloudflareStorageHost<SharedHost> {
    type Database = sqlx_durable_object::Do;
    type Store = RuntimeStore<SharedHost>;
    type Wake = AlarmOwner;

    fn store(&self) -> &Arc<Self::Store> {
        &self.store
    }
    fn wake(&self) -> &Self::Wake {
        &self.wake
    }
    fn with_local_section<Output: Send + 'static>(
        &self,
        run: impl Future<Output = Output> + Send + 'static,
    ) -> impl Future<Output = Result<Output, LocalSectionFailure>> + Send {
        SendFuture::new(async move {
            // Store and per-socket errors remain values. Only platform failure
            // escapes this outer callback and can terminate the whole object.
            self.state
                .block_concurrency_while(async move { Ok(run.await) })
                .await
                .map_err(|_| LocalSectionFailure::Interrupted)
        })
    }
    fn quarantine_connections(
        &self,
        connections: Vec<ConnectionId>,
    ) -> impl Future<Output = ()> + Send {
        // Invalidation must not retry a failed attachment serialization.
        match self.quarantine.lock() {
            Ok(mut quarantine) => quarantine.extend(connections),
            Err(poisoned) => poisoned.into_inner().extend(connections),
        }
        ready(())
    }
    fn send_frame(
        &self,
        to: ConnectionId,
        frame: Bytes,
    ) -> impl Future<Output = Result<(), SocketFailure>> + Send {
        ready((|| {
            let socket = self.socket(to)?;
            if !Self::is_open(&socket) || self.is_quarantined(to) {
                return Err(SocketFailure::Closed);
            }
            socket
                .send_with_bytes(frame)
                .map_err(|_| SocketFailure::Closed)
        })())
    }
    fn close_connection(
        &self,
        to: ConnectionId,
        reason: CloseReason,
    ) -> impl Future<Output = Result<(), SocketFailure>> + Send {
        ready((|| {
            let socket = self.socket(to)?;
            let reason = serde_json::to_string(&reason).map_err(|_| SocketFailure::Closed)?;
            socket
                .close(Some(1000), Some(reason))
                .map_err(|_| SocketFailure::Closed)
        })())
    }
    fn registrations(
        &self,
    ) -> impl Future<Output = Result<RegistrationSnapshot, SocketFailure>> + Send {
        ready({
            let mut registrations = RegistrationSnapshot::new();
            let mut present = BTreeSet::new();
            for socket in self.state.get_websockets() {
                if !Self::is_open(&socket) {
                    continue;
                }
                let attachment = match Self::attachment(&socket) {
                    Ok(attachment) => attachment,
                    Err(failure) => {
                        worker::console_warn!(
                            "isolating websocket with invalid admission attachment: {failure}"
                        );
                        let _ = socket.close(Some(1008), Some("invalid admission attachment"));
                        continue;
                    }
                };
                present.insert(attachment.connection);
                let registration =
                    if Self::is_open(&socket) && !self.is_quarantined(attachment.connection) {
                        attachment.registration
                    } else {
                        ConnectionRegistration::Closed
                    };
                if registrations
                    .insert(attachment.connection, registration)
                    .is_some()
                {
                    // Neither socket may impersonate the duplicate identity;
                    // their failure must not discard unrelated registrations.
                    worker::console_warn!("isolating duplicate websocket connection identity");
                    let _ = socket.close(Some(1008), Some("duplicate connection identity"));
                    self.quarantine
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .insert(attachment.connection);
                    registrations.insert(attachment.connection, ConnectionRegistration::Closed);
                }
            }
            match self.quarantine.lock() {
                Ok(mut quarantine) => quarantine.retain(|connection| present.contains(connection)),
                Err(poisoned) => poisoned
                    .into_inner()
                    .retain(|connection| present.contains(connection)),
            }
            Ok(registrations)
        })
    }
    fn admission_origin(
        &self,
        connection: ConnectionId,
    ) -> impl Future<Output = Result<AdmissionOrigin, SocketFailure>> + Send {
        ready((|| {
            let socket = self.socket(connection)?;
            if !Self::is_open(&socket) || self.is_quarantined(connection) {
                return Err(SocketFailure::Closed);
            }
            self.socket_origin(&socket)
        })())
    }
    fn admission_binding(
        &self,
        connection: ConnectionId,
    ) -> impl Future<Output = Result<ConnectionBinding, SocketFailure>> + Send {
        ready((|| {
            let socket = self.socket(connection)?;
            if !Self::is_open(&socket) || self.is_quarantined(connection) {
                return Err(SocketFailure::Closed);
            }
            self.binding(&socket)
        })())
    }
    fn publish_registrations(
        &self,
        edits: RegistrationEdits,
    ) -> impl Future<Output = Vec<(ConnectionId, SocketFailure)>> + Send {
        ready(self.publish(edits))
    }
}
