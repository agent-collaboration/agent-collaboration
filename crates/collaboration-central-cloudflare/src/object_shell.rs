//! Durable Object construction, physical addressing and hibernatable sockets.

use crate::{CloudflareStorageHost, binding_attachment::BindingAttachment, cloudflare_transport};
use collaboration_central_core::{
    CoreHost, FleetKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
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

pub(crate) struct CloudflareObjectIo {
    env: Arc<SendWrapper<Env>>,
    wake: AlarmOwner,
}
impl Clock for CloudflareObjectIo {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(chrono::Utc::now().timestamp_millis())
    }
}
impl IdSource for CloudflareObjectIo {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
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

    fn object_call(
        &self,
        address: ObjectAddress,
        body: Vec<u8>,
    ) -> impl Future<Output = std::result::Result<Vec<u8>, CallFailure>> + Send {
        SendFuture::new(async move {
            let answer = cloudflare_transport::call(&self.env, address, body).await?;
            Ok(answer)
        })
    }
}
type StoreHost = CloudflareStorageHost<CoreHost<CloudflareObjectIo>>;
type FleetRuntime = ObjectRuntime<FleetKind, StoreHost, CloudflareObjectIo>;
struct ShellActor {
    state: Arc<SendWrapper<State>>,
    env: Arc<SendWrapper<Env>>,
    host: OnceCell<Arc<StoreHost>>,
    runtime: OnceCell<Arc<FleetRuntime>>,
}
impl ShellActor {
    fn new(state: State, env: Env) -> Self {
        Self {
            state: Arc::new(SendWrapper::new(state)),
            env: Arc::new(SendWrapper::new(env)),
            host: OnceCell::new(),
            runtime: OnceCell::new(),
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
        let shared_host = move |wake| {
            Arc::new(CoreHost::for_store(
                CloudflareObjectIo { env, wake },
                config.authoritative_receiver,
            ))
        };
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
    fn runtime_handles(&self) -> Result<(Arc<StoreHost>, Arc<FleetRuntime>)> {
        let config = self.config()?;
        let host = self.host(config)?;
        if let Some(runtime) = self.runtime.get() {
            return Ok((host, runtime.clone()));
        }
        let io = CloudflareObjectIo {
            env: self.env.clone(),
            wake: host.wake().clone(),
        };
        let runtime = Arc::new(
            ObjectRuntime::fleet(host.clone(), io, config, self.delivery_config()?)
                .map_err(error)?,
        );
        // Publish before an initializer can yield, so independent socket events
        // and RPCs share one transient reply hub and one wake owner.
        self.runtime
            .set(runtime.clone())
            .map_err(|_| error("object runtime already constructed"))?;
        Ok((host, runtime))
    }
    async fn runtime(&self) -> Result<(Arc<StoreHost>, Arc<FleetRuntime>)> {
        self.rearm_stale_alarm().await?;
        let (host, runtime) = self.runtime_handles()?;
        host.migrate_store().await.map_err(error)?;
        runtime.initialize().await.map_err(error)?;
        Ok((host, runtime))
    }
    async fn rearm_stale_alarm(&self) -> Result<()> {
        let storage = self.state.storage();
        // Repair at event entry, before any store or runtime can ask the shared
        // library owner for a wake. The platform gate excludes competing events
        // from this read/replace; future alarms keep the library's arbitration.
        let repair = async move {
            let existing = storage.get_alarm().await?;
            let now = chrono::Utc::now();
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
        let address = self.address(Some(requested)).await?;
        let (host, runtime) = self.runtime().await?;
        match request.path().as_str() {
            "/call" => match runtime.on_call(request.bytes().await?).await {
                Ok(bytes) => Response::from_bytes(bytes),
                Err(failure) => Ok(Response::from_json(
                    &cloudflare_transport::WireCallFailure::from(failure),
                )?
                .with_status(409)),
            },
            "/session" => {
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
        self.address(None).await?;
        let (host, runtime) = self.runtime().await?;
        let binding = host.binding(&socket).map_err(error)?;
        let bytes = match message {
            WebSocketIncomingMessage::Binary(bytes) => bytes,
            WebSocketIncomingMessage::String(text) => text.into_bytes(),
        };
        runtime.on_frame(binding, bytes).await.map_err(error)
    }
    async fn disconnected(&self, socket: WebSocket) -> Result<()> {
        self.address(None).await?;
        let (host, runtime) = self.runtime().await?;
        runtime
            .on_socket_closed(host.binding(&socket).map_err(error)?)
            .await
            .map_err(error)
    }
    async fn alarm(&self) -> Result<Response> {
        self.address(None).await?;
        let (_, runtime) = self.runtime().await?;
        runtime.on_alarm().await.map_err(error)?;
        Response::ok("alarm applied")
    }
}
fn error(error: impl std::fmt::Display) -> Error {
    Error::RustError(error.to_string())
}
macro_rules! shell {
    ($name:ident) => {
        #[durable_object]
        pub struct $name {
            actor: ShellActor,
        }
        impl DurableObject for $name {
            fn new(state: State, env: Env) -> Self {
                Self {
                    actor: ShellActor::new(state, env),
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
shell!(FleetObject);
