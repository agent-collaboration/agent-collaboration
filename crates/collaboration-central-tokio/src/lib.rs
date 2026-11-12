//! Tokio capabilities for the runtime-independent central core.
#![forbid(unsafe_code)]

mod access_crypto;
mod native_logging;
mod native_objects;
mod native_server;
mod object_keys;
mod platform;
mod self_hosted_issuer;
pub use self_hosted_issuer::{IssuerError, SelfHostedIssuer};
mod socket_host;
pub use collaboration_central_core::DeliveryConfig;
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
    wake: Arc<SharedHost>,
    sockets: socket_host::TokioSockets,
}

impl<SharedHost: Host> TokioStorageHost<SharedHost> {
    pub fn new(object: Arc<TokioObject<SharedHost>>) -> Self {
        let wake = object.host();
        Self {
            object,
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
    type Store = TokioObject<SharedHost>;
    type Wake = SharedHost;

    fn store(&self) -> &Arc<Self::Store> {
        &self.object
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
