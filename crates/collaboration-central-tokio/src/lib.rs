//! Tokio capabilities for the runtime-independent central core.
#![forbid(unsafe_code)]
#[cfg(feature = "test-support")]
pub mod test_support;

mod access_crypto;
#[cfg(feature = "conformance")]
mod conformance_control;
#[cfg(feature = "conformance")]
mod conformance_store;
mod native_logging;
mod native_objects;
mod native_server;
mod object_alarm_timer;
mod object_keys;
pub use object_alarm_timer::ObjectAlarmTimer;
mod platform;
mod self_hosted_issuer;
pub use self_hosted_issuer::{IssuerError, SelfHostedIssuer};
mod socket_host;
#[cfg(feature = "conformance")]
mod startup_controls;
pub use collaboration_central_core::DeliveryConfig;
pub use native_objects::TokioObjectIo;
#[cfg(feature = "conformance")]
pub use native_objects::{HealthFileSample, SocketOwnerSnapshot};
pub use native_server::{TokioServer, TokioServerError, TokioSettings};
pub use platform::{TokioCentralPlatform, TokioPlatformError};
pub use socket_host::SocketCommand;

use collaboration_central_core::{CoreHost, ObjectAlarm, ObjectIo, ObjectPlatform};
use durable_steps::Host;
use fleet_tokio_objects::TokioObject;
use std::sync::Arc;

/// One object's platform. `SharedHost` is the durable-steps host its journal
/// scopes use; `Io` is the object's own clock, ids, deadlines and calls;
/// `Alarm` is its one alarm, shared with whatever delivers it.
pub struct TokioStorageHost<
    Io: ObjectIo,
    SharedHost: Host = CoreHost<Io>,
    Alarm: ObjectAlarm = ObjectAlarmTimer,
> {
    object: Arc<TokioObject<SharedHost>>,
    #[cfg(feature = "conformance")]
    observed: Arc<
        collaboration_central_core::DriveTokenObserver<
            conformance_store::ConformanceStore<TokioObject<SharedHost>>,
        >,
    >,
    #[cfg(feature = "conformance")]
    faults: Arc<collaboration_central_conformance::RuntimeFaults>,
    alarm: Arc<Alarm>,
    io: Arc<Io>,
    sockets: socket_host::TokioSockets,
}

impl<Io: ObjectIo, SharedHost: Host, Alarm: ObjectAlarm> TokioStorageHost<Io, SharedHost, Alarm> {
    /// The socket table's registrations with their peers (S8 snapshot fence).
    #[cfg(feature = "conformance")]
    pub(crate) async fn registration_rows(
        &self,
    ) -> Vec<collaboration_central_core::conformance_snapshot::RegistrationRow> {
        self.sockets.registration_rows().await
    }

    /// `alarm` is the object's one timer, shared with the alarm loop that waits on it.
    pub fn new(object: Arc<TokioObject<SharedHost>>, io: Arc<Io>, alarm: Arc<Alarm>) -> Self {
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
            alarm,
            io,
            sockets: Default::default(),
        }
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn with_faults(
        object: Arc<TokioObject<SharedHost>>,
        faults: Arc<collaboration_central_conformance::RuntimeFaults>,
        io: Arc<Io>,
        alarm: Arc<Alarm>,
    ) -> Self {
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
            alarm,
            io,
            sockets: Default::default(),
        }
    }

    pub async fn attach_socket(
        &self,
        binding: fleet_session::ConnectionBinding,
        admission_origin: collaboration_central_core::AdmissionOrigin,
    ) -> tokio::sync::mpsc::Receiver<SocketCommand> {
        self.sockets.attach(binding, admission_origin).await
    }

    pub(crate) async fn detach_socket(&self, connection: fleet_protocol::ConnectionId) {
        self.sockets.detach(connection).await;
    }
}

impl<Io: ObjectIo, SharedHost: Host, Alarm: ObjectAlarm> ObjectPlatform
    for TokioStorageHost<Io, SharedHost, Alarm>
{
    type Database = sqlx_turso::Turso;
    #[cfg(not(feature = "conformance"))]
    type Store = TokioObject<SharedHost>;
    #[cfg(feature = "conformance")]
    type Store = collaboration_central_core::DriveTokenObserver<
        conformance_store::ConformanceStore<TokioObject<SharedHost>>,
    >;
    type Alarm = Alarm;
    type Io = Io;

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
    fn io(&self) -> &Arc<Io> {
        &self.io
    }
    fn alarm(&self) -> &Alarm {
        self.alarm.as_ref()
    }
    async fn with_local_section<Output: Send + 'static>(
        &self,
        run: collaboration_central_core::LocalSection<Output>,
    ) -> Result<Output, collaboration_central_core::LocalSectionFailure> {
        let exclusion = self.object.engine_lock();
        let _guard = exclusion.lock().await;
        Ok(run.await)
    }
    async fn quarantine_connections(&self, connections: Vec<fleet_protocol::ConnectionId>) {
        self.sockets.quarantine(connections).await;
    }
    async fn send_frame(
        &self,
        to: fleet_protocol::ConnectionId,
        frame: fleet_session::Bytes,
    ) -> Result<(), collaboration_central_core::SocketFailure> {
        #[cfg(feature = "conformance")]
        if let Ok(identity) = serde_json::from_slice::<
            collaboration_central_conformance::OutboundFrameIdentity,
        >(&frame)
            && let Some(frame) = identity.answer_frame()
            && self
                .faults
                .drop_answer(to, frame)
                .map_err(|_| collaboration_central_core::SocketFailure::Closed)?
        {
            return Ok(());
        }
        #[cfg(feature = "conformance")]
        let sent = frame.clone();
        self.sockets.send(to, SocketCommand::Frame(frame)).await?;
        #[cfg(feature = "conformance")]
        collaboration_central_core::event_trace::record_send(to, &sent);
        Ok(())
    }
    async fn close_connection(
        &self,
        to: fleet_protocol::ConnectionId,
        reason: fleet_protocol::CloseReason,
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
        connection: fleet_protocol::ConnectionId,
    ) -> Result<
        collaboration_central_core::AdmissionOrigin,
        collaboration_central_core::SocketFailure,
    > {
        self.sockets.admission_origin(connection).await
    }
    async fn admission_binding(
        &self,
        connection: fleet_protocol::ConnectionId,
    ) -> Result<fleet_session::ConnectionBinding, collaboration_central_core::SocketFailure> {
        self.sockets.admission_binding(connection).await
    }
    async fn publish_registrations(
        &self,
        edits: collaboration_central_core::RegistrationEdits,
    ) -> Vec<(
        fleet_protocol::ConnectionId,
        collaboration_central_core::SocketFailure,
    )> {
        self.sockets.publish(edits).await
    }
}
