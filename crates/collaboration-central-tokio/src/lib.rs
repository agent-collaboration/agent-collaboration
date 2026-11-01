//! Tokio capabilities for the runtime-independent central core.
#![forbid(unsafe_code)]

mod access_crypto;
#[cfg(feature = "conformance")]
mod conformance_control;
#[cfg(feature = "conformance")]
mod conformance_store;
mod issuer_crypto;
mod native_logging;
mod native_objects;
mod native_server;
mod object_keys;
mod platform;
mod self_hosted_issuer;
pub use self_hosted_issuer::{IssuerError, SelfHostedIssuer};
mod socket_host;
#[cfg(feature = "conformance")]
mod startup_controls;
pub use collaboration_central_core::DeliveryConfig;
#[cfg(feature = "conformance")]
pub use native_objects::HealthFileSample;
pub use native_objects::TokioObjectIo;
pub use native_server::{TokioServer, TokioServerError, TokioSettings};
pub use platform::{TokioCentralPlatform, TokioPlatformError};
pub use socket_host::SocketCommand;

use collaboration_central_core::ObjectHost;
use durable_steps::Host;
use fleet_sync_tokio_runner::TokioObject;
use std::sync::Arc;

pub struct TokioStorageHost<SharedHost: Host> {
    object: Arc<TokioObject<SharedHost>>,
    #[cfg(feature = "conformance")]
    observed: Arc<
        collaboration_central_core::DriveTokenObserver<
            conformance_store::ConformanceStore<TokioObject<SharedHost>>,
        >,
    >,
    #[cfg(feature = "conformance")]
    faults: Arc<collaboration_central_conformance::RuntimeFaults>,
    wake: Arc<SharedHost>,
    sockets: socket_host::TokioSockets,
}

impl<SharedHost: Host> TokioStorageHost<SharedHost> {
    pub fn new(object: Arc<TokioObject<SharedHost>>) -> Self {
        let wake = object.host();
        #[cfg(feature = "conformance")]
        let faults = Arc::new(collaboration_central_conformance::RuntimeFaults::default());
        #[cfg(feature = "conformance")]
        let observed = Arc::new(collaboration_central_core::DriveTokenObserver::new(
            Arc::new(conformance_store::ConformanceStore::new(
                object.clone(),
                faults.clone(),
            )),
        ));
        Self {
            object,
            #[cfg(feature = "conformance")]
            observed,
            #[cfg(feature = "conformance")]
            faults,
            wake,
            sockets: Default::default(),
        }
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn with_faults(
        object: Arc<TokioObject<SharedHost>>,
        faults: Arc<collaboration_central_conformance::RuntimeFaults>,
    ) -> Self {
        let wake = object.host();
        let observed = Arc::new(collaboration_central_core::DriveTokenObserver::new(
            Arc::new(conformance_store::ConformanceStore::new(
                object.clone(),
                faults.clone(),
            )),
        ));
        Self {
            object,
            observed,
            faults,
            wake,
            sockets: Default::default(),
        }
    }

    pub async fn attach_socket(
        &self,
        binding: fleet_sync_session::ConnectionBinding,
        admission_origin: collaboration_central_core::AdmissionOrigin,
    ) -> tokio::sync::mpsc::Receiver<SocketCommand> {
        self.sockets.attach(binding, admission_origin).await
    }

    pub(crate) async fn detach_socket(&self, connection: fleet_sync_protocol::ConnectionId) {
        self.sockets.detach(connection).await;
    }
}

impl<SharedHost: Host> ObjectHost for TokioStorageHost<SharedHost> {
    type Database = sqlx_turso::Turso;
    #[cfg(not(feature = "conformance"))]
    type Store = TokioObject<SharedHost>;
    #[cfg(feature = "conformance")]
    type Store = collaboration_central_core::DriveTokenObserver<
        conformance_store::ConformanceStore<TokioObject<SharedHost>>,
    >;
    type Wake = SharedHost;

    fn store(&self) -> &Arc<Self::Store> {
        #[cfg(feature = "conformance")]
        {
            &self.observed
        }
        #[cfg(not(feature = "conformance"))]
        {
            &self.object
        }
    }
    #[cfg(feature = "conformance")]
    fn stale_drive_rejections(
        &self,
    ) -> Result<Vec<durable_steps::OperationId>, durable_steps::StoreError> {
        self.observed.rejected_operations()
    }
    fn wake(&self) -> &Self::Wake {
        self.wake.as_ref()
    }
    async fn with_local_section<Output: Send + 'static>(
        &self,
        run: impl std::future::Future<Output = Output> + Send + 'static,
    ) -> Result<Output, collaboration_central_core::LocalSectionFailure> {
        let exclusion = self.object.engine_lock();
        let _guard = exclusion.lock().await;
        Ok(run.await)
    }
    async fn quarantine_connections(&self, connections: Vec<fleet_sync_protocol::ConnectionId>) {
        self.sockets.quarantine(connections).await;
    }
    async fn send_frame(
        &self,
        to: fleet_sync_protocol::ConnectionId,
        frame: fleet_sync_session::Bytes,
    ) -> Result<(), collaboration_central_core::SocketFailure> {
        #[cfg(feature = "conformance")]
        if let Ok(identity) = serde_json::from_slice::<
            collaboration_central_conformance::OutboundFrameIdentity,
        >(&frame)
            && let Some(frame) = identity.answer_frame()
            && self
                .faults
                .drop_answer(frame)
                .map_err(|_| collaboration_central_core::SocketFailure::Closed)?
        {
            return Ok(());
        }
        self.sockets.send(to, SocketCommand::Frame(frame)).await
    }
    async fn close_connection(
        &self,
        to: fleet_sync_protocol::ConnectionId,
        reason: fleet_sync_protocol::CloseReason,
    ) -> Result<(), collaboration_central_core::SocketFailure> {
        self.sockets.send(to, SocketCommand::Close(reason)).await
    }
    async fn registrations(
        &self,
    ) -> Result<
        collaboration_central_core::RegistrationSnapshot,
        collaboration_central_core::SocketFailure,
    > {
        Ok(self.sockets.registrations().await)
    }
    async fn admission_origin(
        &self,
        connection: fleet_sync_protocol::ConnectionId,
    ) -> Result<
        collaboration_central_core::AdmissionOrigin,
        collaboration_central_core::SocketFailure,
    > {
        self.sockets.admission_origin(connection).await
    }
    async fn admission_binding(
        &self,
        connection: fleet_sync_protocol::ConnectionId,
    ) -> Result<fleet_sync_session::ConnectionBinding, collaboration_central_core::SocketFailure>
    {
        self.sockets.admission_binding(connection).await
    }
    async fn publish_registrations(
        &self,
        edits: collaboration_central_core::RegistrationEdits,
    ) -> Vec<(
        fleet_sync_protocol::ConnectionId,
        collaboration_central_core::SocketFailure,
    )> {
        self.sockets.publish(edits).await
    }
}
