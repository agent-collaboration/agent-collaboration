//! Physical object hosting, one timer per object and native socket tasks.
mod health_read;
#[cfg(feature = "conformance")]
pub use health_read::HealthFileSample;

use crate::object_keys::{address_from_key, address_key};

use crate::{SocketCommand, TokioStorageHost};
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use collaboration_central_core::{
    AdmissionOrigin, CopyKind, CoreHost, FleetKind, MailboxKind, ObjectAddress, ObjectRuntime,
    PlatformIo, RegistryKind, SessionKind,
};
use durable_steps::{BoxFuture, CallFailure, NotSentReason, StoreError};
use fleet_sync_lease::RegistryConfig;
use fleet_sync_session::ConnectionBinding;
use fleet_sync_tokio_runner::{ObjectKey, ObjectOptions, ObjectStore};
use runtime_primitives::{Clock, IdSource, SyncTime, TokioWakeTimer, WakeFailure, WakeScheduler};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, OnceLock, Weak},
};
use tokio::sync::{Mutex, OnceCell};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

type NativeHost = TokioStorageHost<CoreHost<TokioObjectIo>>;
type RegistryRuntime = ObjectRuntime<RegistryKind, NativeHost, TokioObjectIo>;
type SessionRuntime = ObjectRuntime<SessionKind, NativeHost, TokioObjectIo>;
type CopyRuntime = ObjectRuntime<CopyKind, NativeHost, TokioObjectIo>;
type FleetRuntime = ObjectRuntime<FleetKind, NativeHost, TokioObjectIo>;
type MailboxRuntime = ObjectRuntime<MailboxKind, NativeHost, TokioObjectIo>;

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
    wake: Arc<TokioWakeTimer<SharedClock>>,
    #[cfg(feature = "conformance")]
    faults: Arc<collaboration_central_conformance::RuntimeFaults>,
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
impl PlatformIo for TokioObjectIo {
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
    Fleet(FleetRuntime),
    Registry(RegistryRuntime),
    Session(SessionRuntime),
    Copy(CopyRuntime),
    Mailbox(MailboxRuntime),
}
impl RuntimeFace {
    async fn initialize(&self) -> Result<(), StoreError> {
        match self {
            Self::Fleet(runtime) => runtime.initialize().await.map(|_| ()),
            Self::Registry(runtime) => runtime.initialize().await.map(|_| ()),
            Self::Session(runtime) => runtime.initialize().await.map(|_| ()),
            // Copy's core call/alarm prelude runs after Health classification.
            Self::Copy(_) => Ok(()),
            Self::Mailbox(runtime) => runtime.initialize().await.map(|_| ()),
        }
    }
    // Keep a scheduled drive's SQL and dispatched effects in one observable
    // scope, distinct from inbound requests and later redelivery attempts.
    #[tracing::instrument(level = "trace", name = "central_alarm", skip_all, fields(object_kind = match self {
        Self::Fleet(_) => "fleet", Self::Registry(_) => "registry", Self::Session(_) => "session",
        Self::Copy(_) => "copy", Self::Mailbox(_) => "mailbox",
    }))]
    async fn on_alarm(&self) -> Result<(), StoreError> {
        match self {
            Self::Fleet(runtime) => runtime.on_alarm().await.map(|_| ()),
            Self::Registry(runtime) => runtime.on_alarm().await.map(|_| ()),
            Self::Session(runtime) => runtime.on_alarm().await.map(|_| ()),
            Self::Copy(runtime) => runtime.on_alarm().await,
            Self::Mailbox(runtime) => runtime.on_alarm().await.map(|_| ()),
        }
    }
}

struct ObjectSlot {
    io: TokioObjectIo,
    host: OnceCell<Arc<NativeHost>>,
    runtime: OnceCell<Arc<RuntimeFace>>,
    alarm_task: OnceLock<()>,
    #[cfg(test)]
    alarm_owner_ready: tokio::sync::watch::Sender<bool>,
    #[cfg(feature = "conformance")]
    startup_alarm_completed: tokio::sync::watch::Sender<bool>,
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
    health_samples: Mutex<BTreeMap<fleet_sync_protocol::DatabaseId, Vec<HealthFileSample>>>,
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
        if matches!(
            request.command,
            collaboration_central_conformance::RuntimeControl::SmallMailboxCapacity
        ) {
            let Some(runtime) = slot.runtime.get() else {
                return Err(StoreError::backend("mailbox runtime not initialized"));
            };
            let RuntimeFace::Mailbox(mailbox) = runtime.as_ref() else {
                return Err(StoreError::backend("small capacity requires a mailbox"));
            };
            mailbox
                .conformance_small_mailbox_capacity()
                .map_err(StoreError::backend)?;
        }
        let observed = slot
            .io
            .faults
            .control(request.command)
            .await
            .map_err(StoreError::backend)?;
        if advances_clock {
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
                            wake: Arc::new(TokioWakeTimer::new(self.clock.clone())),
                            #[cfg(feature = "conformance")]
                            faults: self.startup_controls.faults_for(&key),
                        },
                        host: OnceCell::new(),
                        runtime: OnceCell::new(),
                        alarm_task: OnceLock::new(),
                        #[cfg(test)]
                        alarm_owner_ready: tokio::sync::watch::channel(false).0,
                        #[cfg(feature = "conformance")]
                        startup_alarm_completed: tokio::sync::watch::channel(false).0,
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
                let host = TokioStorageHost::with_faults(object, slot.io.faults.clone());
                #[cfg(not(feature = "conformance"))]
                let host = TokioStorageHost::new(object);
                Ok::<_, StoreError>(Arc::new(host))
            })
            .await?
            .clone();
        let runtime = slot
            .runtime
            .get_or_try_init(|| async {
                let runtime = match &address {
                    ObjectAddress::Registry => RuntimeFace::Registry(ObjectRuntime::registry(
                        host,
                        slot.io.clone(),
                        self.config,
                        self.delivery,
                    )?),
                    ObjectAddress::Connection(installation) => {
                        RuntimeFace::Session(ObjectRuntime::session(
                            host,
                            slot.io.clone(),
                            self.config.authoritative_receiver,
                            installation.clone(),
                            self.holder_answer_timeout,
                        )?)
                    }
                    ObjectAddress::Copy(project) => RuntimeFace::Copy(ObjectRuntime::copy(
                        host,
                        slot.io.clone(),
                        self.config.authoritative_receiver,
                        *project,
                        self.copy_ack_wait,
                    )),
                    ObjectAddress::Mailbox(installation) => {
                        let limits = collaboration_central_core::central_mailbox_limits()
                            .map_err(StoreError::backend)?;
                        let mailbox = ObjectRuntime::mailbox(
                            host,
                            slot.io.clone(),
                            self.config.authoritative_receiver,
                            installation.clone(),
                            limits,
                        )?;
                        #[cfg(feature = "conformance")]
                        if slot
                            .io
                            .faults
                            .small_mailbox_capacity()
                            .map_err(StoreError::backend)?
                        {
                            mailbox
                                .conformance_small_mailbox_capacity()
                                .map_err(StoreError::backend)?;
                        }
                        RuntimeFace::Mailbox(mailbox)
                    }
                    ObjectAddress::Fleet => RuntimeFace::Fleet(ObjectRuntime::fleet(
                        host,
                        slot.io.clone(),
                        self.config.authoritative_receiver,
                    )?),
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
            let ready = slot.alarm_owner_ready.clone();
            #[cfg(feature = "conformance")]
            let startup_completed = slot.startup_alarm_completed.clone();
            self.tasks.spawn(alarm_owner::run(
                runtime,
                wake,
                clock,
                stop,
                #[cfg(test)]
                ready,
                #[cfg(feature = "conformance")]
                startup_completed,
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
            match slot.runtime.get().map(Arc::as_ref) {
                Some(RuntimeFace::Fleet(runtime)) => runtime.on_call(body).await,
                Some(RuntimeFace::Registry(runtime)) => runtime.on_call(body).await,
                Some(RuntimeFace::Copy(runtime)) => match runtime.copy_call_class(&body)? {
                    collaboration_central_core::CopyCallClass::Health => {
                        objects
                            .read_health(
                                &slot,
                                runtime,
                                body,
                                #[cfg(feature = "conformance")]
                                health_address,
                            )
                            .await
                    }
                    collaboration_central_core::CopyCallClass::Ordinary => {
                        runtime.on_call(body).await
                    }
                },
                Some(RuntimeFace::Mailbox(runtime)) => runtime.on_call(body).await,
                Some(RuntimeFace::Session(runtime)) => {
                    runtime
                        .delivery()
                        .map_err(|_| no_route())?
                        .on_call(body)
                        .await
                }
                _ => Err(no_route()),
            }
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
        let Ok(slot) = self
            .slot(ObjectAddress::Connection(origin.installation.clone()))
            .await
        else {
            return;
        };
        let Some(host) = slot.host.get() else {
            return;
        };
        let Some(runtime_handle) = slot.runtime.get().cloned() else {
            return;
        };
        let RuntimeFace::Session(runtime) = runtime_handle.as_ref() else {
            return;
        };
        let mut outgoing = host.attach_socket(binding, origin).await;
        let mut socket = socket;
        let mut requests = tokio::task::JoinSet::new();
        if runtime.on_socket_opened(binding).await.is_ok() {
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
                            requests.spawn(async move { match runtime.as_ref() { RuntimeFace::Session(runtime) => runtime.on_frame(binding, bytes.to_vec()).await, _ => Err(StoreError::Aborted { reason: "session runtime missing".into() }) } });
                        },
                        Some(Ok(Message::Text(text))) => {
                            let runtime = runtime_handle.clone();
                            requests.spawn(async move { match runtime.as_ref() { RuntimeFace::Session(runtime) => runtime.on_frame(binding, text.as_bytes().to_vec()).await, _ => Err(StoreError::Aborted { reason: "session runtime missing".into() }) } });
                        },
                        Some(Ok(Message::Ping(_) | Message::Pong(_))) => {},
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    },
                }
            }
        }
        requests.abort_all();
        while requests.join_next().await.is_some() {}
        let _ = runtime.on_socket_closed(binding).await;
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
        service: &message_board::ServiceId,
    ) -> Result<collaboration_central_core::ForwardStateObservation, StoreError> {
        let slot = self
            .slots
            .lock()
            .await
            .get(&format!("connection-{}", service.as_str()))
            .cloned();
        match slot
            .as_ref()
            .and_then(|slot| slot.runtime.get())
            .map(Arc::as_ref)
        {
            Some(RuntimeFace::Session(runtime)) => runtime.conformance_forward_state().await,
            _ => Err(StoreError::Aborted {
                reason: "connection runtime missing".into(),
            }),
        }
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn holder_frames(
        &self,
        service: &message_board::ServiceId,
    ) -> Result<Vec<fleet_sync_protocol::FrameId>, StoreError> {
        let slot = self
            .slots
            .lock()
            .await
            .get(&format!("connection-{}", service.as_str()))
            .cloned();
        match slot
            .as_ref()
            .and_then(|slot| slot.runtime.get())
            .map(Arc::as_ref)
        {
            Some(RuntimeFace::Session(runtime)) => runtime.conformance_holder_frames(),
            _ => Err(StoreError::Aborted {
                reason: "connection runtime missing".into(),
            }),
        }
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn socket_origins(
        &self,
        service: &message_board::ServiceId,
    ) -> Result<Vec<AdmissionOrigin>, collaboration_central_core::SocketFailure> {
        use collaboration_central_core::ObjectHost;
        let slot = self
            .slots
            .lock()
            .await
            .get(&format!("connection-{}", service.as_str()))
            .cloned();
        let host = slot
            .as_ref()
            .and_then(|slot| slot.host.get())
            .ok_or(collaboration_central_core::SocketFailure::NotConnected)?;
        let mut origins = Vec::new();
        for connection in host.registrations().await?.keys() {
            origins.push(host.admission_origin(*connection).await?);
        }
        Ok(origins)
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn schema_report(
        self: &Arc<Self>,
        peer: fleet_sync_protocol::PeerId,
    ) -> Result<Option<fleet_sync_protocol::ReportSchemaSupport>, StoreError> {
        let slot = self.slot(ObjectAddress::Registry).await?;
        match slot.runtime.get().map(Arc::as_ref) {
            Some(RuntimeFace::Registry(runtime)) => runtime.conformance_schema_report(peer).await,
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

fn no_route() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}

#[path = "native_objects/alarm_owner.rs"]
mod alarm_owner;
#[cfg(all(test, feature = "conformance"))]
#[path = "native_objects/alarm_recovery_tests.rs"]
mod alarm_recovery_tests;
#[cfg(feature = "conformance")]
#[path = "conformance_board_import.rs"]
mod conformance_board_import;
#[cfg(feature = "conformance")]
mod startup_alarm;
