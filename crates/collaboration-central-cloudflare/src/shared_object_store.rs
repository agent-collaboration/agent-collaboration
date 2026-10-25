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

#[cfg(feature = "conformance")]
#[path = "shared_object_store/session_capabilities.rs"]
mod session_capabilities;

#[cfg(feature = "conformance")]
type SharedStore<SharedHost> = crate::conformance_scope_faults::ScopeFaultStore<SharedHost>;
#[cfg(not(feature = "conformance"))]
type SharedStore<SharedHost> = DurableObjectStore<SharedHost>;

#[cfg(feature = "conformance")]
type RuntimeStore<SharedHost> =
    collaboration_central_core::DriveTokenObserver<SharedStore<SharedHost>>;
#[cfg(not(feature = "conformance"))]
type RuntimeStore<SharedHost> = SharedStore<SharedHost>;

pub struct CloudflareStorageHost<SharedHost: Host> {
    store: Arc<RuntimeStore<SharedHost>>,
    wake: AlarmOwner,
    state: Arc<SendWrapper<State>>,
    quarantine: Mutex<BTreeSet<ConnectionId>>,
    #[cfg(feature = "conformance")]
    faults: Arc<collaboration_central_conformance::RuntimeFaults>,
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
        #[cfg(feature = "conformance")]
        let faults = Arc::new(collaboration_central_conformance::RuntimeFaults::default());
        #[cfg(feature = "conformance")]
        let store =
            crate::conformance_scope_faults::ScopeFaultStore::with_faults(store, faults.clone());
        let store = Arc::new(store);
        #[cfg(feature = "conformance")]
        let store = Arc::new(collaboration_central_core::DriveTokenObserver::new(store));
        Self {
            store,
            wake,
            state,
            quarantine: Mutex::default(),
            #[cfg(feature = "conformance")]
            faults,
        }
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn with_faults(
        state: Arc<SendWrapper<State>>,
        shared_host: impl FnOnce(AlarmOwner) -> Arc<SharedHost>,
        faults: Arc<collaboration_central_conformance::RuntimeFaults>,
    ) -> Self {
        let wake = AlarmOwner::new(state.storage());
        let shared_host = shared_host(wake.clone());
        let store = DurableObjectStore::new(
            sqlx_durable_object::DoConnection::new(state.storage()),
            shared_host,
            wake.clone(),
        );
        let store = Arc::new(
            crate::conformance_scope_faults::ScopeFaultStore::with_faults(store, faults.clone()),
        );
        let store = Arc::new(collaboration_central_core::DriveTokenObserver::new(store));
        Self {
            store,
            wake,
            state,
            quarantine: Mutex::default(),
            faults,
        }
    }

    pub async fn migrate_store(&self) -> Result<(), durable_steps::StoreError> {
        #[cfg(feature = "conformance")]
        {
            self.store.inner().migrate().await
        }
        #[cfg(not(feature = "conformance"))]
        {
            self.store.migrate().await
        }
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
                #[cfg(feature = "conformance")]
                if matches!(registration, ConnectionRegistration::Attached { .. })
                    && self
                        .faults
                        .fail_registration_publication(connection)
                        .map_err(|_| SocketFailure::Closed)?
                {
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
    #[cfg(feature = "conformance")]
    fn session_finalize_fault(
        &self,
    ) -> Result<Option<collaboration_central_core::SessionFinalizeFault>, durable_steps::StoreError>
    {
        self.faults
            .session_finalize_fault()
            .map_err(durable_steps::StoreError::backend)
    }
    #[cfg(feature = "conformance")]
    fn observe_session_finalize_failure(
        &self,
        failure: collaboration_central_core::SessionFinalizeFailure,
    ) -> Result<(), durable_steps::StoreError> {
        self.faults
            .observe_session_finalize_failure(failure)
            .map_err(durable_steps::StoreError::backend)
    }
    #[cfg(feature = "conformance")]
    fn stale_drive_rejections(
        &self,
    ) -> Result<Vec<durable_steps::OperationId>, durable_steps::StoreError> {
        self.store.rejected_operations()
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
            #[cfg(feature = "conformance")]
            if let Ok(identity) = serde_json::from_slice::<
                collaboration_central_conformance::OutboundFrameIdentity,
            >(&frame)
                && let Some(frame) = identity.answer_frame()
                && self
                    .faults
                    .drop_answer(frame)
                    .map_err(|_| SocketFailure::Closed)?
            {
                return Ok(());
            }
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
        ready((|| {
            let mut registrations = RegistrationSnapshot::new();
            let mut present = BTreeSet::new();
            for socket in self.state.get_websockets() {
                let attachment = Self::attachment(&socket)?;
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
                    return Err(SocketFailure::Closed);
                }
            }
            match self.quarantine.lock() {
                Ok(mut quarantine) => quarantine.retain(|connection| present.contains(connection)),
                Err(poisoned) => poisoned
                    .into_inner()
                    .retain(|connection| present.contains(connection)),
            }
            Ok(registrations)
        })())
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
