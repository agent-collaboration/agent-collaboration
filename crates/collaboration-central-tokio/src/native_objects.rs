//! Physical object hosting, one timer per object and native socket tasks.
use crate::object_keys::{address_from_key, address_key};

use crate::{SocketCommand, TokioStorageHost};
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use collaboration_central_core::{
    AdmissionOrigin, CoreHost, FleetKind, ObjectAddress, ObjectRuntime, PlatformIo,
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
type FleetRuntime = ObjectRuntime<FleetKind, NativeHost, TokioObjectIo>;

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
}
impl Clock for TokioObjectIo {
    fn now(&self) -> SyncTime {
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
    fn reply_deadline(
        &self,
        remaining: std::time::Duration,
    ) -> impl std::future::Future<Output = ()> + Send {
        tokio::time::sleep(remaining)
    }

    async fn object_call(
        &self,
        address: ObjectAddress,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        let objects = self.objects.upgrade().ok_or_else(no_route)?;
        let answer = objects.call(address, body).await?;
        Ok(answer)
    }
}

struct ObjectSlot {
    io: TokioObjectIo,
    host: OnceCell<Arc<NativeHost>>,
    runtime: OnceCell<Arc<FleetRuntime>>,
    alarm_task: OnceLock<()>,
}

pub(crate) struct NativeObjects {
    store: ObjectStore<CoreHost<TokioObjectIo>>,
    slots: Mutex<BTreeMap<String, Arc<ObjectSlot>>>,
    clock: SharedClock,
    config: RegistryConfig,
    delivery: crate::DeliveryConfig,
    pub(crate) stop: CancellationToken,
    pub(crate) tasks: TaskTracker,
}
impl NativeObjects {
    pub(crate) async fn open(
        root: &Path,
        clock: Arc<dyn Clock>,
        config: RegistryConfig,
        delivery: crate::DeliveryConfig,
    ) -> Result<Arc<Self>, StoreError> {
        let objects = Arc::new(Self {
            store: ObjectStore::open(root).await?,
            slots: Mutex::default(),
            clock: SharedClock(clock),
            config,
            delivery,
            stop: CancellationToken::new(),
            tasks: TaskTracker::new(),
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
                objects.slot(address_from_key(key)?).await?;
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
                        },
                        host: OnceCell::new(),
                        runtime: OnceCell::new(),
                        alarm_task: OnceLock::new(),
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
                        ObjectOptions { foreign_keys: true },
                        Arc::new(CoreHost::for_store(
                            slot.io.clone(),
                            self.config.authoritative_receiver,
                        )),
                    )
                    .await?;
                let host = TokioStorageHost::new(object);
                Ok::<_, StoreError>(Arc::new(host))
            })
            .await?
            .clone();
        let runtime = slot
            .runtime
            .get_or_try_init(|| async {
                let runtime =
                    ObjectRuntime::fleet(host, slot.io.clone(), self.config, self.delivery)?;
                runtime.initialize().await?;
                Ok::<_, StoreError>(Arc::new(runtime))
            })
            .await?
            .clone();
        slot.alarm_task.get_or_init(|| {
            let wake = slot.io.wake.clone();
            let clock = self.clock.clone();
            let stop = self.stop.clone();
            self.tasks
                .spawn(alarm_owner::run(runtime, wake, clock, stop));
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
            let slot = objects.slot(address).await.map_err(|_| no_route())?;
            match slot.runtime.get() {
                Some(runtime) => runtime.on_call(body).await,
                None => Err(no_route()),
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
        let Ok(slot) = self.slot(ObjectAddress::Fleet).await else {
            return;
        };
        let Some(host) = slot.host.get() else {
            return;
        };
        let Some(runtime) = slot.runtime.get().cloned() else {
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
                            let runtime = runtime.clone();
                            requests.spawn(async move { runtime.on_frame(binding, bytes.to_vec()).await });
                        },
                        Some(Ok(Message::Text(text))) => {
                            let runtime = runtime.clone();
                            requests.spawn(async move { runtime.on_frame(binding, text.as_bytes().to_vec()).await });
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
