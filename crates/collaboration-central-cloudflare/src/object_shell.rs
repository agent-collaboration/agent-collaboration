//! Durable Object construction, physical addressing and hibernatable sockets.

use crate::worker_clock::CloudflareClock;
use crate::{CloudflareStorageHost, binding_attachment::BindingAttachment, cloudflare_transport};
use collaboration_central_core::ClockIds;
use collaboration_central_core::{
    CopyKind, CoreHost, FleetKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
};
use durable_steps::{CallFailure, NotSentReason};
use fleet_sync_lease::RegistryConfig;
use runtime_primitives::{AlarmOwner, Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use std::{cell::OnceCell, future::Future, sync::Arc};
use worker::{
    DurableObject, Env, Error, Request, Response, Result, State, WebSocket,
    WebSocketIncomingMessage, WebSocketPair, durable_object,
    send::{SendFuture, SendWrapper},
    wasm_bindgen,
};

#[cfg(feature = "conformance")]
#[path = "object_shell/conformance_routes.rs"]
mod conformance_routes;

pub(crate) struct CloudflareObjectIo {
    env: Arc<SendWrapper<Env>>,
    wake: AlarmOwner,
    #[cfg(feature = "conformance")]
    faults: Arc<collaboration_central_conformance::RuntimeFaults>,
    ids: Arc<ClockIds<CloudflareClock>>,
}
impl Clock for CloudflareObjectIo {
    fn now(&self) -> SyncTime {
        #[cfg(feature = "conformance")]
        if let Some(now) = self.faults.clock() {
            return SyncTime::from_unix_millis(now.0.timestamp_millis());
        }
        CloudflareClock.now()
    }
}
impl IdSource for CloudflareObjectIo {
    fn new_id(&self) -> durable_steps::Uuid {
        self.ids.new_id()
    }
}
impl WakeScheduler for CloudflareObjectIo {
    fn wake_at(
        &self,
        at: SyncTime,
    ) -> impl Future<Output = std::result::Result<(), WakeFailure>> + Send {
        self.wake.wake_at(at)
    }
}
impl PlatformIo for CloudflareObjectIo {
    fn reply_deadline(
        &self,
        remaining: std::time::Duration,
    ) -> impl std::future::Future<Output = ()> + Send {
        worker::send::SendFuture::new(crate::reply_deadline::ReplyDeadline::new(remaining))
    }

    #[cfg(feature = "conformance")]
    async fn before_local_call(&self, request: &[u8]) -> std::result::Result<(), CallFailure> {
        self.faults
            .before_dispatch(&ObjectAddress::Fleet, request)
            .await
            .map_err(|_| CallFailure::AnswerLost)
    }
    #[cfg(feature = "conformance")]
    fn drop_local_reply(&self, request: &[u8]) -> std::result::Result<bool, CallFailure> {
        self.faults
            .drop_successful_object_reply(&ObjectAddress::Fleet, request)
            .map_err(|_| CallFailure::AnswerLost)
    }
    fn object_call(
        &self,
        address: ObjectAddress,
        body: Vec<u8>,
    ) -> impl Future<Output = std::result::Result<Vec<u8>, CallFailure>> + Send {
        SendFuture::new(async move {
            #[cfg(feature = "conformance")]
            self.faults
                .before_dispatch(&address, &body)
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
            #[cfg(feature = "conformance")]
            let original_destination = address.clone();
            #[cfg(feature = "conformance")]
            let original_request = body.clone();
            let answer = cloudflare_transport::call(&self.env, address, body).await?;
            #[cfg(feature = "conformance")]
            if self
                .faults
                .drop_successful_object_reply(&original_destination, &original_request)
                .map_err(|_| CallFailure::AnswerLost)?
            {
                return Err(CallFailure::AnswerLost);
            }
            Ok(answer)
        })
    }
}
type StoreHost = CloudflareStorageHost<CoreHost<CloudflareObjectIo>>;
enum RuntimeFace {
    Fleet(ObjectRuntime<FleetKind, StoreHost, CloudflareObjectIo>),
    Copy(ObjectRuntime<CopyKind, StoreHost, CloudflareObjectIo>),
}
impl RuntimeFace {
    async fn initialize(&self) -> Result<()> {
        match self {
            Self::Fleet(runtime) => runtime.initialize().await.map_err(error)?,
            // Copy's core classifies Health before its call/alarm prelude.
            Self::Copy(_) => 0,
        };
        Ok(())
    }
    async fn on_alarm(&self) -> Result<()> {
        match self {
            Self::Fleet(runtime) => {
                runtime.on_alarm().await.map_err(error)?;
            }
            Self::Copy(runtime) => runtime.on_alarm().await.map_err(error)?,
        };
        Ok(())
    }
}
#[derive(Clone, Copy)]
enum PhysicalKind {
    Copy,
    Fleet,
}
impl PhysicalKind {
    fn matches(self, address: &ObjectAddress) -> bool {
        matches!(
            (self, address),
            (Self::Copy, ObjectAddress::Copy(_)) | (Self::Fleet, ObjectAddress::Fleet)
        )
    }
}
struct ShellActor {
    state: Arc<SendWrapper<State>>,
    env: Arc<SendWrapper<Env>>,
    kind: PhysicalKind,
    host: OnceCell<Arc<StoreHost>>,
    runtime: OnceCell<Arc<RuntimeFace>>,
    #[cfg(feature = "conformance")]
    faults: Arc<collaboration_central_conformance::RuntimeFaults>,
    ids: Arc<ClockIds<CloudflareClock>>,
}
impl ShellActor {
    fn new(state: State, env: Env, kind: PhysicalKind) -> Self {
        Self {
            state: Arc::new(SendWrapper::new(state)),
            env: Arc::new(SendWrapper::new(env)),
            kind,
            host: OnceCell::new(),
            runtime: OnceCell::new(),
            #[cfg(feature = "conformance")]
            faults: Arc::default(),
            ids: Arc::new(ClockIds::new(CloudflareClock)),
        }
    }
    fn config(&self) -> Result<RegistryConfig> {
        serde_json::from_str(&self.env.var("CENTRAL_REGISTRY_CONFIG")?.to_string())
            .map_err(Into::into)
    }
    fn delivery_config(&self) -> Result<collaboration_central_core::DeliveryConfig> {
        serde_json::from_str(&self.env.var("CENTRAL_DELIVERY_CONFIG")?.to_string())
            .map_err(Into::into)
    }
    fn host(&self, config: RegistryConfig) -> Result<Arc<StoreHost>> {
        if let Some(host) = self.host.get() {
            return Ok(host.clone());
        }
        let env = self.env.clone();
        #[cfg(feature = "conformance")]
        let faults = self.faults.clone();
        let ids = self.ids.clone();
        let shared_host = move |wake| {
            Arc::new(CoreHost::for_store(
                CloudflareObjectIo {
                    env,
                    ids,
                    wake,
                    #[cfg(feature = "conformance")]
                    faults,
                },
                config.authoritative_receiver,
            ))
        };
        #[cfg(feature = "conformance")]
        let host = Arc::new(CloudflareStorageHost::with_faults(
            self.state.clone(),
            shared_host,
            self.faults.clone(),
        ));
        #[cfg(not(feature = "conformance"))]
        let host = Arc::new(CloudflareStorageHost::new(self.state.clone(), shared_host));
        // Publish the one wake owner before any initializer can yield.
        self.host
            .set(host.clone())
            .map_err(|_| error("object host already constructed"))?;
        Ok(host)
    }
    async fn address(&self, requested: Option<ObjectAddress>) -> Result<ObjectAddress> {
        let storage = self.state.storage();
        let stored = storage.get::<ObjectAddress>("_cf_actor_address").await?;
        let address = match (stored, requested.as_ref()) {
            (Some(stored), Some(expected)) if stored != *expected => {
                return Err(error("object address mismatch"));
            }
            (Some(stored), _) => stored,
            (None, Some(expected)) => expected.clone(),
            (None, None) => return Err(error("object address missing")),
        };
        if !self.kind.matches(&address) {
            return Err(error("wrong physical object kind"));
        }
        let (binding, name) = cloudflare_transport::location(&address);
        if self
            .env
            .durable_object(binding)?
            .id_from_name(&name)?
            .to_string()
            != self.state.id().to_string()
        {
            return Err(error("object address does not match physical id"));
        }
        if requested.is_some() {
            storage.put("_cf_actor_address", &address).await?;
        }
        Ok(address)
    }
    fn runtime_handles(
        &self,
        address: ObjectAddress,
    ) -> Result<(Arc<StoreHost>, Arc<RuntimeFace>)> {
        let config = self.config()?;
        let host = self.host(config)?;
        if let Some(runtime) = self.runtime.get() {
            return Ok((host, runtime.clone()));
        }
        let io = CloudflareObjectIo {
            env: self.env.clone(),
            wake: host.wake().clone(),
            #[cfg(feature = "conformance")]
            faults: self.faults.clone(),
            ids: self.ids.clone(),
        };
        let runtime = match address {
            ObjectAddress::WorkspaceStore(_) => {
                return Err(error("workspace store takes only store calls"));
            }
            ObjectAddress::Copy(project) => RuntimeFace::Copy(ObjectRuntime::copy(
                host.clone(),
                io,
                config.authoritative_receiver,
                project,
                serde_json::from_str(&self.env.var("CENTRAL_COPY_ACK_WAIT")?.to_string())?,
            )),
            ObjectAddress::Fleet => RuntimeFace::Fleet(
                ObjectRuntime::fleet(
                    host.clone(),
                    io,
                    config,
                    self.delivery_config()?,
                    serde_json::from_str(
                        &self.env.var("CENTRAL_HOLDER_ANSWER_TIMEOUT")?.to_string(),
                    )?,
                )
                .map_err(error)?,
            ),
        };
        let runtime = Arc::new(runtime);
        // Publish before an initializer can yield, so independent socket events
        // and RPCs share one transient reply hub and one wake owner.
        self.runtime
            .set(runtime.clone())
            .map_err(|_| error("object runtime already constructed"))?;
        Ok((host, runtime))
    }
    #[cfg(feature = "conformance")]
    async fn restore_conformance_state(&self) -> Result<()> {
        let now = self
            .state
            .storage()
            .get::<collaboration_central_core::RegistryTime>(crate::composed_control::CLOCK_KEY)
            .await?;
        self.faults.restore_clock(now).map_err(error)?;
        let unavailable = self
            .state
            .storage()
            .get::<bool>(crate::composed_control::PROJECT_FAULT_KEY)
            .await?
            .unwrap_or(false);
        self.faults
            .restore_project_fault(unavailable)
            .map_err(error)?;
        Ok(())
    }
    async fn runtime(&self, address: ObjectAddress) -> Result<(Arc<StoreHost>, Arc<RuntimeFace>)> {
        #[cfg(feature = "conformance")]
        self.restore_conformance_state().await?;
        self.rearm_stale_alarm().await?;
        let (host, runtime) = self.runtime_handles(address)?;
        host.migrate_store().await.map_err(error)?;
        // After the journal's own migrations: on a fresh object the journal refuses
        // to adopt tables it did not create first.
        host.migrate_statements(crate::object_migrations::central_object())
            .await
            .map_err(error)?;
        runtime.initialize().await?;
        Ok((host, runtime))
    }
    async fn rearm_stale_alarm(&self) -> Result<()> {
        #[cfg(feature = "conformance")]
        let faults = self.faults.clone();
        let storage = self.state.storage();
        // Repair at event entry, before any store or runtime can ask the shared
        // library owner for a wake. The platform gate excludes competing events
        // from this read/replace; future alarms keep the library's arbitration.
        let repair = async move {
            let existing = storage.get_alarm().await?;
            let now = chrono::Utc::now();
            #[cfg(feature = "conformance")]
            let now = faults.clock().map_or(now, |now| now.0);
            if existing.is_some_and(|at| at < now.timestamp_millis()) {
                storage.set_alarm(worker::ScheduledTime::from(now)).await?;
            }
            Ok::<_, Error>(())
        };
        // Match with_local_section: an I/O error is a returned value, not a
        // thrown blockConcurrencyWhile callback that resets the whole object.
        self.state
            .block_concurrency_while(async move { Ok(repair.await) })
            .await?
    }
    async fn fetch(&self, mut request: Request) -> Result<Response> {
        let requested = request
            .headers()
            .get("x-central-object-address")?
            .ok_or_else(|| error("object address missing"))?;
        let requested: ObjectAddress = serde_json::from_str(&requested)?;
        #[cfg(feature = "conformance")]
        let read_only = request.path() == "/test/fleet-mail-custody";
        #[cfg(feature = "conformance")]
        let address = if read_only {
            let stored = self.address(None).await?;
            if stored != requested {
                return Err(error("object address mismatch"));
            }
            stored
        } else {
            self.address(Some(requested)).await?
        };
        #[cfg(not(feature = "conformance"))]
        let address = self.address(Some(requested)).await?;
        #[cfg(feature = "conformance")]
        if request.path() == crate::composed_control::OBJECT_PATH
            || request.path() == "/test/fleet-schema"
            || request.path() == "/test/fleet-mail-custody"
        {
            self.restore_conformance_state().await?;
            let (host, runtime) = self.runtime_handles(address.clone())?;
            if let Some(response) = self
                .conformance_fetch(&mut request, host.as_ref(), runtime.as_ref())
                .await?
            {
                return Ok(response);
            }
        }
        #[cfg(feature = "conformance")]
        if request.path() == "/call" && matches!(address, ObjectAddress::Copy(_)) {
            let unavailable = self
                .state
                .storage()
                .get::<bool>(crate::composed_control::PROJECT_FAULT_KEY)
                .await?
                .unwrap_or(false);
            if unavailable {
                return Ok(Response::from_json(
                    &cloudflare_transport::WireCallFailure::Unreachable { last_seen: None },
                )?
                .with_status(409));
            }
        }
        let (host, runtime) = self.runtime(address.clone()).await?;
        #[cfg(feature = "conformance")]
        if let Some(response) = self
            .conformance_fetch(&mut request, host.as_ref(), runtime.as_ref())
            .await?
        {
            return Ok(response);
        }
        match (request.path().as_str(), runtime.as_ref()) {
            ("/call", RuntimeFace::Fleet(runtime)) => {
                match runtime.on_call(request.bytes().await?).await {
                    Ok(bytes) => Response::from_bytes(bytes),
                    Err(failure) => Ok(Response::from_json(
                        &cloudflare_transport::WireCallFailure::from(failure),
                    )?
                    .with_status(409)),
                }
            }
            ("/call", RuntimeFace::Copy(runtime)) => {
                let bytes = request.bytes().await?;
                let result = match runtime.copy_call_class(&bytes) {
                    Ok(collaboration_central_core::CopyCallClass::Health) => {
                        match crate::storage_measurement::storage_bytes(&self.state.storage()) {
                            Ok(storage_bytes) => runtime.on_health(bytes, storage_bytes).await,
                            Err(_) => Err(CallFailure::AnswerLost),
                        }
                    }
                    Ok(collaboration_central_core::CopyCallClass::Ordinary) => {
                        runtime.on_call(bytes).await
                    }
                    Err(failure) => Err(failure),
                };
                match result {
                    Ok(bytes) => Response::from_bytes(bytes),
                    Err(failure) => Ok(Response::from_json(
                        &cloudflare_transport::WireCallFailure::from(failure),
                    )?
                    .with_status(409)),
                }
            }
            ("/session", RuntimeFace::Fleet(runtime)) => {
                use base64::Engine;
                let encoded = request
                    .headers()
                    .get("x-central-session-binding")?
                    .ok_or_else(|| error("session binding missing"))?;
                let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(encoded)
                    .map_err(error)?;
                let attachment: BindingAttachment = serde_json::from_slice(&bytes)?;
                if address != ObjectAddress::Fleet
                    || attachment.registration
                        != fleet_sync_session::ConnectionRegistration::Opening
                {
                    return Err(error("session binding address mismatch"));
                }
                let binding = attachment.binding();
                let pair = WebSocketPair::new()?;
                host.accept_socket(&pair.server, binding, attachment.admission_origin)
                    .map_err(error)?;
                runtime.on_socket_opened(binding).await.map_err(error)?;
                Response::from_websocket(pair.client)
            }
            _ => Ok(
                Response::from_json(&cloudflare_transport::WireCallFailure::from(
                    CallFailure::NotSent {
                        reason: NotSentReason::Unreachable { last_seen: None },
                    },
                ))?
                .with_status(409),
            ),
        }
    }
    async fn websocket_message(
        &self,
        socket: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        let address = self.address(None).await?;
        let (host, runtime) = self.runtime(address).await?;
        let RuntimeFace::Fleet(runtime) = runtime.as_ref() else {
            return Err(error("socket on non-session object"));
        };
        let binding = host.binding(&socket).map_err(error)?;
        let bytes = match message {
            WebSocketIncomingMessage::Binary(bytes) => bytes,
            WebSocketIncomingMessage::String(text) => text.into_bytes(),
        };
        runtime.on_frame(binding, bytes).await.map_err(error)
    }
    async fn disconnected(&self, socket: WebSocket) -> Result<()> {
        let address = self.address(None).await?;
        let (host, runtime) = self.runtime(address).await?;
        let RuntimeFace::Fleet(runtime) = runtime.as_ref() else {
            return Err(error("socket on non-session object"));
        };
        runtime
            .on_socket_closed(host.binding(&socket).map_err(error)?)
            .await
            .map_err(error)
    }
    async fn alarm(&self) -> Result<Response> {
        let address = self.address(None).await?;
        let (_, runtime) = self.runtime(address).await?;
        runtime.on_alarm().await?;
        Response::ok("alarm applied")
    }
}
fn error(error: impl std::fmt::Display) -> Error {
    Error::RustError(error.to_string())
}
macro_rules! shell {
    ($name:ident, $kind:ident) => {
        #[durable_object]
        pub struct $name {
            actor: ShellActor,
        }
        impl DurableObject for $name {
            fn new(state: State, env: Env) -> Self {
                Self {
                    actor: ShellActor::new(state, env, PhysicalKind::$kind),
                }
            }
            async fn fetch(&self, request: Request) -> Result<Response> {
                self.actor.fetch(request).await
            }
            async fn alarm(&self) -> Result<Response> {
                self.actor.alarm().await
            }
            async fn websocket_message(
                &self,
                socket: WebSocket,
                message: WebSocketIncomingMessage,
            ) -> Result<()> {
                self.actor.websocket_message(socket, message).await
            }
            async fn websocket_close(
                &self,
                socket: WebSocket,
                _: usize,
                _: String,
                _: bool,
            ) -> Result<()> {
                self.actor.disconnected(socket).await
            }
            async fn websocket_error(&self, socket: WebSocket, _: Error) -> Result<()> {
                self.actor.disconnected(socket).await
            }
        }
    };
}
shell!(ProjectCopyObject, Copy);
shell!(FleetObject, Fleet);
