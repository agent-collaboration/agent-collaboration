//! Physical object hosting, one timer per object and native socket tasks.
mod health_read;
#[cfg(feature = "conformance")]
pub use health_read::HealthFileSample;

#[cfg(feature = "conformance")]
#[derive(Clone, Debug)]
pub struct SocketOwnerSnapshot {
    pub address: collaboration_central_core::ObjectAddress,
    pub admissions: Vec<collaboration_central_core::AdmissionOrigin>,
}

#[cfg(feature = "conformance")]
#[derive(serde::Serialize)]
pub(crate) struct SessionConnectionSnapshot {
    connection: fleet_protocol::ConnectionId,
    peer: fleet_protocol::PeerId,
    registration: fleet_session::ConnectionRegistration,
}

use crate::object_keys::{address_from_key, address_key};

use crate::ObjectAlarmTimer;
use crate::{SocketCommand, TokioStorageHost};
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use collaboration_central_core::{
    AdmissionOrigin, CopyKind, CoreHost, EventReply, FleetKind, ObjectAddress, ObjectEvent,
    ObjectIo, ObjectKernel, ObjectRuntime,
};
use durable_steps::{BoxFuture, CallFailure, NotSentReason, StoreError};
use fleet_project_lease::RegistryConfig;
use fleet_session::ConnectionBinding;
use fleet_tokio_objects::{ObjectKey, ObjectOptions, ObjectStore};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, OnceLock, Weak},
};
use tokio::sync::{Mutex, OnceCell};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

type NativeHost = TokioStorageHost<TokioObjectIo>;
type CopyRuntime = ObjectRuntime<CopyKind, NativeHost>;
type FleetRuntime = ObjectRuntime<FleetKind, NativeHost>;

#[derive(Clone)]
struct SharedClock(Arc<dyn Clock>);
impl Clock for SharedClock {
    fn now(&self) -> SyncTime {
        self.0.now()
    }
}

#[derive(Clone)]
pub struct TokioObjectIo {
    objects: Weak<NativeObjects>,
    clock: SharedClock,
    wake: Arc<ObjectAlarmTimer>,
    #[cfg(feature = "conformance")]
    faults: Arc<collaboration_central_conformance::RuntimeFaults>,
    /// Signalled when a conformance control moves the injected clock.
    #[cfg(feature = "conformance")]
    clock_moved: Arc<tokio::sync::watch::Sender<()>>,
}
impl Clock for TokioObjectIo {
    fn now(&self) -> SyncTime {
        #[cfg(feature = "conformance")]
        if let Some(now) = self.faults.clock() {
            return SyncTime::from_unix_millis(now.0.timestamp_millis());
        }
        self.clock.now()
    }
}
impl IdSource for TokioObjectIo {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for TokioObjectIo {
    async fn wake_at(&self, at: SyncTime) -> Result<(), WakeFailure> {
        self.wake.wake_at(at).await
    }
}
impl ObjectIo for TokioObjectIo {
    fn event_deadline(
        &self,
        remaining: std::time::Duration,
    ) -> impl std::future::Future<Output = ()> + Send {
        #[cfg(not(feature = "conformance"))]
        {
            tokio::time::sleep(remaining)
        }
        // Conformance only: the deadline also passes when the injected clock does,
        // so a test ends a wait causally, without real time or an alarm.
        #[cfg(feature = "conformance")]
        {
            let io = self.clone();
            let until = self.now().after(remaining);
            let mut moved = self.clock_moved.subscribe();
            async move {
                let injected = async {
                    while io.now() < until {
                        if moved.changed().await.is_err() {
                            std::future::pending::<()>().await;
                        }
                    }
                };
                tokio::select! {
                    () = tokio::time::sleep(remaining) => {}
                    () = injected => {}
                }
            }
        }
    }

    #[cfg(feature = "conformance")]
    async fn before_local_call(&self, request: &[u8]) -> Result<(), CallFailure> {
        self.faults
            .before_dispatch(&ObjectAddress::Fleet, request)
            .await
            .map_err(|_| CallFailure::AnswerLost)
    }
    #[cfg(feature = "conformance")]
    fn drop_local_reply(&self, request: &[u8]) -> Result<bool, CallFailure> {
        self.faults
            .drop_successful_object_reply(&ObjectAddress::Fleet, request)
            .map_err(|_| CallFailure::AnswerLost)
    }
    async fn object_call(
        &self,
        address: ObjectAddress,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        #[cfg(feature = "conformance")]
        self.faults
            .before_dispatch(&address, &body)
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        let objects = self.objects.upgrade().ok_or_else(no_route)?;
        #[cfg(feature = "conformance")]
        let original_destination = address.clone();
        #[cfg(feature = "conformance")]
        let original_request = body.clone();
        let answer = objects.call(address, body).await?;
        #[cfg(feature = "conformance")]
        if self
            .faults
            .drop_successful_object_reply(&original_destination, &original_request)
            .map_err(|_| CallFailure::AnswerLost)?
        {
            return Err(CallFailure::AnswerLost);
        }
        Ok(answer)
    }
}

enum RuntimeFace {
    Fleet(ObjectKernel<FleetRuntime>),
    Copy(ObjectKernel<CopyRuntime>),
}
impl RuntimeFace {
    async fn initialize(&self) -> Result<(), StoreError> {
        match self {
            Self::Fleet(kernel) => kernel.face().initialize().await.map(|_| ()),
            // Copy's core call/alarm prelude runs after Health classification.
            Self::Copy(_) => Ok(()),
        }
    }
    // Keep each event's SQL and dispatched effects in one observable scope.
    #[tracing::instrument(level = "trace", name = "central_event", skip_all, fields(object_kind = match self {
        Self::Fleet(_) => "fleet",
        Self::Copy(_) => "copy",
    }))]
    async fn on_event(&self, event: ObjectEvent) -> EventReply {
        match self {
            Self::Fleet(kernel) => kernel.on_event(event).await,
            Self::Copy(kernel) => kernel.on_event(event).await,
        }
    }
}

fn call_answer(reply: EventReply) -> Result<Vec<u8>, CallFailure> {
    match reply {
        EventReply::CallAnswer(answer) => Ok(answer),
        EventReply::CallFailure(failure) => Err(failure),
        EventReply::None
        | EventReply::SocketFailed
        | EventReply::AlarmDone
        | EventReply::AlarmRetry => Err(CallFailure::AnswerLost),
    }
}

struct ObjectSlot {
    io: TokioObjectIo,
    host: OnceCell<Arc<NativeHost>>,
    runtime: OnceCell<Arc<RuntimeFace>>,
    alarm_task: OnceLock<()>,
    #[cfg(test)]
    alarm_owner_generation: tokio::sync::watch::Sender<u64>,
}

pub(crate) struct NativeObjects {
    store: ObjectStore<CoreHost<TokioObjectIo>>,
    slots: Mutex<BTreeMap<String, Arc<ObjectSlot>>>,
    clock: SharedClock,
    config: RegistryConfig,
    delivery: crate::DeliveryConfig,
    holder_answer_timeout: std::time::Duration,
    copy_ack_wait: std::time::Duration,
    pub(crate) stop: CancellationToken,
    pub(crate) tasks: TaskTracker,
    #[cfg(feature = "conformance")]
    startup_controls: crate::startup_controls::PreparedRuntimeControls,
    #[cfg(feature = "conformance")]
    health_samples: Mutex<BTreeMap<fleet_protocol::DatabaseId, Vec<HealthFileSample>>>,
}
impl NativeObjects {
    #[cfg(feature = "conformance")]
    pub(crate) async fn runtime_control(
        self: &Arc<Self>,
        request: collaboration_central_conformance::RuntimeControlRequest,
    ) -> Result<collaboration_central_conformance::RuntimeFaultObservation, StoreError> {
        request.validate().map_err(StoreError::backend)?;
        let advances_clock = matches!(
            request.command,
            collaboration_central_conformance::RuntimeControl::SetTime { .. }
        );
        let slot = self.slot(request.address).await?;
        let observed = slot
            .io
            .faults
            .control(request.command)
            .await
            .map_err(StoreError::backend)?;
        if advances_clock {
            slot.io.clock_moved.send_replace(());
            slot.io
                .wake
                .wake_at(self.clock.now())
                .await
                .map_err(StoreError::backend)?;
        }
        Ok(observed)
    }
    #[cfg(not(feature = "conformance"))]
    pub(crate) async fn open(
        root: &Path,
        clock: Arc<dyn Clock>,
        config: RegistryConfig,
        delivery: crate::DeliveryConfig,
        holder_answer_timeout: std::time::Duration,
        copy_ack_wait: std::time::Duration,
    ) -> Result<Arc<Self>, StoreError> {
        Self::open_prepared(
            root,
            clock,
            config,
            delivery,
            holder_answer_timeout,
            copy_ack_wait,
            #[cfg(feature = "conformance")]
            Default::default(),
        )
        .await
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn open_with_controls(
        root: &Path,
        clock: Arc<dyn Clock>,
        config: RegistryConfig,
        delivery: crate::DeliveryConfig,
        holder_answer_timeout: std::time::Duration,
        copy_ack_wait: std::time::Duration,
        controls: Vec<collaboration_central_conformance::RuntimeControlRequest>,
    ) -> Result<Arc<Self>, StoreError> {
        let prepared = crate::startup_controls::PreparedRuntimeControls::prepare(controls).await?;
        Self::open_prepared(
            root,
            clock,
            config,
            delivery,
            holder_answer_timeout,
            copy_ack_wait,
            prepared,
        )
        .await
    }

    async fn open_prepared(
        root: &Path,
        clock: Arc<dyn Clock>,
        config: RegistryConfig,
        delivery: crate::DeliveryConfig,
        holder_answer_timeout: std::time::Duration,
        copy_ack_wait: std::time::Duration,
        #[cfg(feature = "conformance")]
        startup_controls: crate::startup_controls::PreparedRuntimeControls,
    ) -> Result<Arc<Self>, StoreError> {
        let objects = Arc::new(Self {
            store: ObjectStore::open(root).await?,
            slots: Mutex::default(),
            clock: SharedClock(clock),
            config,
            delivery,
            holder_answer_timeout,
            copy_ack_wait,
            stop: CancellationToken::new(),
            tasks: TaskTracker::new(),
            #[cfg(feature = "conformance")]
            startup_controls,
            #[cfg(feature = "conformance")]
            health_samples: Mutex::default(),
        });
        let mut files = tokio::fs::read_dir(root)
            .await
            .map_err(StoreError::backend)?;
        while let Some(file) = files.next_entry().await.map_err(StoreError::backend)? {
            let path = file.path();
            if path.extension().is_some_and(|extension| extension == "db") {
                let key = path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| StoreError::Aborted {
                        reason: "invalid native object filename".into(),
                    })?;
                let address = address_from_key(key)?;
                let copy = matches!(address, ObjectAddress::Copy(_));
                let slot = objects.slot(address).await?;
                if copy {
                    slot.io
                        .wake_at(slot.io.now())
                        .await
                        .map_err(StoreError::backend)?;
                }
            }
        }
        Ok(objects)
    }

    async fn slot(self: &Arc<Self>, address: ObjectAddress) -> Result<Arc<ObjectSlot>, StoreError> {
        let key = address_key(&address)?;
        let slot = {
            let mut slots = self.slots.lock().await;
            slots
                .entry(key.clone())
                .or_insert_with(|| {
                    Arc::new(ObjectSlot {
                        io: TokioObjectIo {
                            objects: Arc::downgrade(self),
                            clock: self.clock.clone(),
                            wake: Arc::new(ObjectAlarmTimer::new(self.clock.0.clone())),
                            #[cfg(feature = "conformance")]
                            faults: self.startup_controls.faults_for(&key),
                            #[cfg(feature = "conformance")]
                            clock_moved: Arc::new(tokio::sync::watch::channel(()).0),
                        },
                        host: OnceCell::new(),
                        runtime: OnceCell::new(),
                        alarm_task: OnceLock::new(),
                        #[cfg(test)]
                        alarm_owner_generation: tokio::sync::watch::channel(0_u64).0,
                    })
                })
                .clone()
        };
        // The slot and its timer exist before initialization can yield. Retry
        // after a cancelled initializer therefore reuses the same wake owner.
        let host = slot
            .host
            .get_or_try_init(|| async {
                let object = self
                    .store
                    .object(
                        ObjectKey::new(key.clone()).map_err(StoreError::backend)?,
                        ObjectOptions {
                            foreign_keys: !matches!(&address, ObjectAddress::Copy(_)),
                        },
                        Arc::new(CoreHost::for_store(
                            slot.io.clone(),
                            self.config.authoritative_receiver,
                        )),
                    )
                    .await?;
                #[cfg(feature = "conformance")]
                let host = TokioStorageHost::with_faults(
                    object,
                    slot.io.faults.clone(),
                    Arc::new(slot.io.clone()),
                    slot.io.wake.clone(),
                );
                #[cfg(not(feature = "conformance"))]
                let host =
                    TokioStorageHost::new(object, Arc::new(slot.io.clone()), slot.io.wake.clone());
                Ok::<_, StoreError>(Arc::new(host))
            })
            .await?
            .clone();
        let runtime = slot
            .runtime
            .get_or_try_init(|| async {
                let runtime = match &address {
                    ObjectAddress::Copy(project) => {
                        RuntimeFace::Copy(ObjectKernel::new(ObjectRuntime::copy(
                            host,
                            self.config.authoritative_receiver,
                            *project,
                            self.copy_ack_wait,
                        )))
                    }
                    ObjectAddress::Fleet => {
                        RuntimeFace::Fleet(ObjectKernel::new(ObjectRuntime::fleet(
                            host,
                            self.config,
                            self.delivery,
                            self.holder_answer_timeout,
                            fleet_protocol::FrameCapabilities::default(),
                        )?))
                    }
                };
                runtime.initialize().await?;
                Ok::<_, StoreError>(Arc::new(runtime))
            })
            .await?
            .clone();
        slot.alarm_task.get_or_init(|| {
            let wake = slot.io.wake.clone();
            let clock = self.clock.clone();
            let stop = self.stop.clone();
            #[cfg(test)]
            let ready = slot.alarm_owner_generation.clone();
            self.tasks.spawn(alarm_owner::run(
                runtime,
                wake,
                clock,
                stop,
                #[cfg(test)]
                ready,
            ));
        });
        Ok(slot)
    }

    pub(crate) fn call(
        self: &Arc<Self>,
        address: ObjectAddress,
        body: Vec<u8>,
    ) -> BoxFuture<'static, Result<Vec<u8>, CallFailure>> {
        let objects = self.clone();
        Box::pin(async move {
            #[cfg(feature = "conformance")]
            let health_address = address.clone();
            let slot = objects.slot(address).await.map_err(|_| no_route())?;
            #[cfg(feature = "conformance")]
            if slot
                .io
                .faults
                .project_unavailable()
                .map_err(|_| CallFailure::AnswerLost)?
            {
                return Err(no_route());
            }
            let runtime = slot.runtime.get().cloned().ok_or_else(no_route)?;
            let storage_bytes = match runtime.as_ref() {
                RuntimeFace::Copy(kernel) => match kernel.face().copy_call_class(&body)? {
                    collaboration_central_core::CopyCallClass::Health => Some(
                        objects
                            .measure_storage(
                                &slot,
                                #[cfg(feature = "conformance")]
                                health_address,
                            )
                            .await?,
                    ),
                    collaboration_central_core::CopyCallClass::Ordinary => None,
                },
                RuntimeFace::Fleet(_) => None,
            };
            call_answer(
                runtime
                    .on_event(ObjectEvent::Call {
                        body,
                        storage_bytes,
                    })
                    .await,
            )
        })
    }

    pub(crate) async fn serve_socket(
        self: Arc<Self>,
        socket: WebSocket,
        binding: ConnectionBinding,
        origin: AdmissionOrigin,
    ) {
        if self.stop.is_cancelled() {
            return;
        }
        let Ok(slot) = self.slot(ObjectAddress::Fleet).await else {
            return;
        };
        let Some(host) = slot.host.get() else {
            return;
        };
        let Some(runtime_handle) = slot.runtime.get().cloned() else {
            return;
        };
        if !matches!(runtime_handle.as_ref(), RuntimeFace::Fleet(_)) {
            return;
        }
        let mut outgoing = host.attach_socket(binding, origin).await;
        let mut socket = socket;
        let mut requests = tokio::task::JoinSet::new();
        if matches!(
            runtime_handle
                .on_event(ObjectEvent::SocketOpened { binding })
                .await,
            EventReply::None
        ) {
            loop {
                tokio::select! {
                    _ = self.stop.cancelled() => break,
                    joined = requests.join_next(), if !requests.is_empty() => {
                        if !matches!(joined, Some(Ok(Ok(())))) { break; }
                    },
                    command = outgoing.recv() => match command {
                        Some(SocketCommand::Frame(bytes)) => if socket.send(Message::Binary(bytes)).await.is_err() { break; },
                        Some(SocketCommand::Close(reason)) => {
                            if let Ok(reason) = serde_json::to_string(&reason) {
                                let _ = socket.send(Message::Close(Some(CloseFrame { code: 1000, reason: reason.into() }))).await;
                            }
                            break;
                        }
                        None => break,
                    },
                    message = socket.recv() => match message {
                        Some(Ok(Message::Binary(bytes))) => {
                            let runtime = runtime_handle.clone();
                            requests.spawn(async move { frame_outcome(runtime.on_event(ObjectEvent::Frame { binding, frame: bytes.to_vec() }).await) });
                        },
                        Some(Ok(Message::Text(text))) => {
                            let runtime = runtime_handle.clone();
                            requests.spawn(async move { frame_outcome(runtime.on_event(ObjectEvent::Frame { binding, frame: text.as_bytes().to_vec() }).await) });
                        },
                        Some(Ok(Message::Ping(_) | Message::Pong(_))) => {},
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    },
                }
            }
        }
        requests.abort_all();
        while requests.join_next().await.is_some() {}
        let _ = runtime_handle
            .on_event(ObjectEvent::SocketClosed { binding })
            .await;
        host.detach_socket(binding.connection).await;
    }

    pub(crate) async fn shutdown(&self) -> Result<(), StoreError> {
        self.stop.cancel();
        self.tasks.close();
        self.tasks.wait().await;
        self.store.close().await
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn forward_state(
        &self,
        _service: &message_board::ServiceId,
    ) -> Result<collaboration_central_core::ForwardStateObservation, StoreError> {
        let slot = self.slots.lock().await.get("fleet").cloned();
        match slot
            .as_ref()
            .and_then(|slot| slot.runtime.get())
            .map(Arc::as_ref)
        {
            Some(RuntimeFace::Fleet(kernel)) => kernel.face().conformance_forward_state().await,
            _ => Err(StoreError::Aborted {
                reason: "connection runtime missing".into(),
            }),
        }
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn holder_frames(
        &self,
        _service: &message_board::ServiceId,
    ) -> Result<Vec<fleet_protocol::FrameId>, StoreError> {
        let slot = self.slots.lock().await.get("fleet").cloned();
        match slot
            .as_ref()
            .and_then(|slot| slot.runtime.get())
            .map(Arc::as_ref)
        {
            Some(RuntimeFace::Fleet(kernel)) => kernel.face().conformance_holder_frames(),
            _ => Err(StoreError::Aborted {
                reason: "connection runtime missing".into(),
            }),
        }
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn mail_custody(
        &self,
        item: fleet_protocol::ItemId,
    ) -> Result<collaboration_central_core::FleetMailCustodyObservation, StoreError> {
        // Observation never creates or migrates an owning store.
        let slot = self.slots.lock().await.get("fleet").cloned();
        match slot
            .as_ref()
            .and_then(|slot| slot.runtime.get())
            .map(Arc::as_ref)
        {
            Some(RuntimeFace::Fleet(kernel)) => kernel.face().conformance_mail_custody(item).await,
            _ => Err(StoreError::Aborted {
                reason: "Fleet owner missing".into(),
            }),
        }
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn session_connections(
        &self,
        service: Option<&message_board::ServiceId>,
    ) -> Result<Vec<SessionConnectionSnapshot>, collaboration_central_core::SocketFailure> {
        use collaboration_central_core::ObjectPlatform;
        let slot = self.slots.lock().await.get("fleet").cloned();
        let host = slot
            .as_ref()
            .and_then(|slot| slot.host.get())
            .ok_or(collaboration_central_core::SocketFailure::NotConnected)?;
        let mut connections = Vec::new();
        for (connection, registration) in host.registrations().await? {
            let origin = host.admission_origin(connection).await?;
            if service.is_none_or(|service| &origin.installation == service) {
                let binding = host.admission_binding(connection).await?;
                connections.push(SessionConnectionSnapshot {
                    connection,
                    peer: binding.peer,
                    registration,
                });
            }
        }
        Ok(connections)
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn socket_inventory(
        &self,
    ) -> Result<Vec<SocketOwnerSnapshot>, collaboration_central_core::SocketFailure> {
        use collaboration_central_core::ObjectPlatform;
        let slots = self.slots.lock().await.clone();
        let mut inventory = Vec::new();
        for (key, slot) in slots {
            let Some(host) = slot.host.get() else {
                continue;
            };
            let address = address_from_key(&key)
                .map_err(|_| collaboration_central_core::SocketFailure::NotConnected)?;
            let mut admissions = Vec::new();
            for connection in host.registrations().await?.keys() {
                admissions.push(host.admission_origin(*connection).await?);
            }
            inventory.push(SocketOwnerSnapshot {
                address,
                admissions,
            });
        }
        Ok(inventory)
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn socket_origins(
        &self,
        service: &message_board::ServiceId,
    ) -> Result<Vec<AdmissionOrigin>, collaboration_central_core::SocketFailure> {
        use collaboration_central_core::ObjectPlatform;
        let slot = self.slots.lock().await.get("fleet").cloned();
        let host = slot
            .as_ref()
            .and_then(|slot| slot.host.get())
            .ok_or(collaboration_central_core::SocketFailure::NotConnected)?;
        let mut origins = Vec::new();
        for connection in host.registrations().await?.keys() {
            let origin = host.admission_origin(*connection).await?;
            if &origin.installation == service {
                origins.push(origin);
            }
        }
        Ok(origins)
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn schema_report(
        self: &Arc<Self>,
        peer: fleet_protocol::PeerId,
    ) -> Result<Option<fleet_protocol::ReportSchemaSupport>, StoreError> {
        let slot = self.slot(ObjectAddress::Fleet).await?;
        match slot.runtime.get().map(Arc::as_ref) {
            Some(RuntimeFace::Fleet(kernel)) => kernel.face().conformance_schema_report(peer).await,
            _ => Err(StoreError::Aborted {
                reason: "registry runtime missing".into(),
            }),
        }
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn prune_fleet_feed(self: &Arc<Self>) -> Result<(u64, i64), StoreError> {
        use durable_steps::{AtomicStore, storage::TransactionScope};
        use sqlx_core::{query::query, row::Row};
        let slot = self.slot(ObjectAddress::Fleet).await?;
        let host = slot.host.get().ok_or_else(|| StoreError::Aborted {
            reason: "Fleet owner missing".into(),
        })?;
        host.object.transaction(|scope| Box::pin(async move {
            // Keep the actual owner's floor and head coherent after full pruning.
            query::<sqlx_turso::Turso>("UPDATE fleet_feed_state SET retained_through=last_position WHERE name='fleet'")
                .execute(scope.executor()).await.map_err(StoreError::backend)?;
            let deleted = query::<sqlx_turso::Turso>("DELETE FROM fleet_feed")
                .execute(scope.executor()).await.map_err(StoreError::backend)?.rows_affected();
            let row = query::<sqlx_turso::Turso>("SELECT COUNT(*) AS remaining FROM fleet_feed")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((deleted,row.try_get("remaining").map_err(StoreError::backend)?))
        })).await
    }
}
impl Drop for NativeObjects {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// A failed frame ends the socket loop, which closes the connection.
fn frame_outcome(reply: EventReply) -> Result<(), StoreError> {
    match reply {
        EventReply::SocketFailed => Err(StoreError::Aborted {
            reason: "frame failed".into(),
        }),
        _ => Ok(()),
    }
}

fn no_route() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}

#[path = "native_objects/alarm_owner.rs"]
mod alarm_owner;
#[cfg(feature = "conformance")]
#[path = "conformance_board_import.rs"]
mod conformance_board_import;
