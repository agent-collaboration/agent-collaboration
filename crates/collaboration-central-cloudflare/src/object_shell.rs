//! Durable Object construction, physical addressing and hibernatable sockets.

use crate::{CloudflareStorageHost, binding_attachment::BindingAttachment, cloudflare_transport};
#[cfg(feature = "conformance")]
use collaboration_central_core::ObjectPlatform;
use collaboration_central_core::{
    CopyKind, CoreHost, EventReply, FleetKind, ObjectAddress, ObjectEvent, ObjectIo, ObjectKernel,
    ObjectRuntime,
};
use durable_steps::{CallFailure, NotSentReason};
use fleet_project_lease::RegistryConfig;
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
#[cfg(feature = "conformance")]
#[path = "object_shell/event_lines.rs"]
pub(crate) mod event_lines;

pub(crate) struct CloudflareObjectIo {
    env: Arc<SendWrapper<Env>>,
    wake: AlarmOwner,
    #[cfg(feature = "conformance")]
    faults: Arc<collaboration_central_conformance::RuntimeFaults>,
}
impl Clock for CloudflareObjectIo {
    fn now(&self) -> SyncTime {
        #[cfg(feature = "conformance")]
        if let Some(now) = self.faults.clock() {
            return SyncTime::from_unix_millis(now.0.timestamp_millis());
        }
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
        let lowered = self.wake.wake_at(at);
        async move {
            lowered.await?;
            #[cfg(feature = "conformance")]
            collaboration_central_core::event_trace::record_lower(at).await;
            Ok(())
        }
    }
}
impl ObjectIo for CloudflareObjectIo {
    fn event_deadline(
        &self,
        remaining: std::time::Duration,
    ) -> impl std::future::Future<Output = ()> + Send {
        worker::send::SendFuture::new(crate::event_deadline::EventDeadline::new(remaining))
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
            #[cfg(feature = "conformance")]
            let answer = collaboration_central_core::event_trace::traced_call(
                &original_destination,
                &original_request,
                cloudflare_transport::call(&self.env, address, body),
            )
            .await?;
            #[cfg(not(feature = "conformance"))]
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
type StoreHost = CloudflareStorageHost<CloudflareObjectIo>;
enum RuntimeFace {
    Fleet(ObjectKernel<ObjectRuntime<FleetKind, StoreHost>>),
    Copy(ObjectKernel<ObjectRuntime<CopyKind, StoreHost>>),
}
impl RuntimeFace {
    async fn initialize(&self) -> Result<()> {
        match self {
            Self::Fleet(kernel) => kernel.face().initialize().await.map_err(error)?,
            // Copy's core classifies Health before its call/alarm prelude.
            Self::Copy(_) => 0,
        };
        Ok(())
    }
    async fn on_event(&self, event: ObjectEvent) -> EventReply {
        match self {
            Self::Fleet(kernel) => kernel.on_event(event).await,
            Self::Copy(kernel) => kernel.on_event(event).await,
        }
    }
    /// `on_event` with what only the shell knows, recorded in S1.
    #[cfg(feature = "conformance")]
    async fn on_event_with(
        &self,
        event: ObjectEvent,
        context: collaboration_central_core::event_trace::EventContext,
    ) -> EventReply {
        match self {
            Self::Fleet(kernel) => kernel.on_event_with(event, context).await,
            Self::Copy(kernel) => kernel.on_event_with(event, context).await,
        }
    }
}
fn call_response(reply: EventReply) -> Result<Response> {
    match reply {
        EventReply::CallAnswer(bytes) => Response::from_bytes(bytes),
        EventReply::CallFailure(failure) => Ok(Response::from_json(
            &cloudflare_transport::WireCallFailure::from(failure),
        )?
        .with_status(409)),
        EventReply::None
        | EventReply::SocketFailed
        | EventReply::AlarmDone
        | EventReply::AlarmRetry => Ok(Response::from_json(
            &cloudflare_transport::WireCallFailure::from(CallFailure::AnswerLost),
        )?
        .with_status(409)),
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
    /// Minted in this Rust `DurableObject::new`; not proof of a new JS instance.
    #[cfg(feature = "conformance")]
    isolate: durable_steps::Uuid,
    /// This isolate migrated and initialized the store (S8 answers `notOpen`
    /// until then).
    #[cfg(feature = "conformance")]
    opened: std::sync::atomic::AtomicBool,
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
            #[cfg(feature = "conformance")]
            isolate: durable_steps::Uuid::now_v7(),
            #[cfg(feature = "conformance")]
            opened: std::sync::atomic::AtomicBool::new(false),
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
        let shared_host = move |wake| {
            Arc::new(CoreHost::for_store(
                CloudflareObjectIo {
                    env,
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
        #[cfg(feature = "conformance")]
        let observer = event_lines::observer(address.clone(), self.isolate);
        let runtime = match address {
            ObjectAddress::Copy(project) => RuntimeFace::Copy(kernel(
                ObjectRuntime::copy(
                    host.clone(),
                    config.authoritative_receiver,
                    project,
                    serde_json::from_str(&self.env.var("CENTRAL_COPY_ACK_WAIT")?.to_string())?,
                ),
                #[cfg(feature = "conformance")]
                observer,
            )),
            ObjectAddress::Fleet => RuntimeFace::Fleet(kernel(
                ObjectRuntime::fleet(
                    host.clone(),
                    config,
                    self.delivery_config()?,
                    serde_json::from_str(
                        &self.env.var("CENTRAL_HOLDER_ANSWER_TIMEOUT")?.to_string(),
                    )?,
                    collaboration_central_core::CENTRAL_FRAME_CAPABILITIES,
                )
                .map_err(error)?,
                #[cfg(feature = "conformance")]
                observer,
            )),
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
        let (host, runtime) = self.runtime_handles(address)?;
        host.migrate_store().await.map_err(error)?;
        runtime.initialize().await?;
        #[cfg(feature = "conformance")]
        self.opened
            .store(true, std::sync::atomic::Ordering::Release);
        Ok((host, runtime))
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
            ("/call", RuntimeFace::Fleet(_)) => {
                let body = request.bytes().await?;
                call_response(
                    runtime
                        .on_event(ObjectEvent::Call {
                            body,
                            storage_bytes: None,
                        })
                        .await,
                )
            }
            ("/call", RuntimeFace::Copy(kernel)) => {
                let body = request.bytes().await?;
                let storage_bytes = match kernel.face().copy_call_class(&body) {
                    Ok(collaboration_central_core::CopyCallClass::Health) => {
                        match crate::storage_measurement::storage_bytes(&self.state.storage()) {
                            Ok(storage_bytes) => Some(storage_bytes),
                            Err(_) => {
                                return call_response(EventReply::CallFailure(
                                    CallFailure::AnswerLost,
                                ));
                            }
                        }
                    }
                    Ok(collaboration_central_core::CopyCallClass::Ordinary) => None,
                    Err(failure) => return call_response(EventReply::CallFailure(failure)),
                };
                call_response(
                    runtime
                        .on_event(ObjectEvent::Call {
                            body,
                            storage_bytes,
                        })
                        .await,
                )
            }
            ("/session", RuntimeFace::Fleet(_)) => {
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
                    || attachment.registration != fleet_session::ConnectionRegistration::Opening
                {
                    return Err(error("session binding address mismatch"));
                }
                let binding = attachment.binding();
                let pair = WebSocketPair::new()?;
                host.accept_socket(&pair.server, binding, attachment.admission_origin)
                    .map_err(error)?;
                socket_outcome(
                    runtime
                        .on_event(ObjectEvent::SocketOpened { binding })
                        .await,
                )?;
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
        if !matches!(runtime.as_ref(), RuntimeFace::Fleet(_)) {
            return Err(error("socket on non-session object"));
        }
        let binding = host.binding(&socket).map_err(error)?;
        let bytes = match message {
            WebSocketIncomingMessage::Binary(bytes) => bytes,
            WebSocketIncomingMessage::String(text) => text.into_bytes(),
        };
        socket_outcome(
            runtime
                .on_event(ObjectEvent::Frame {
                    binding,
                    frame: bytes,
                })
                .await,
        )
    }
    async fn disconnected(&self, socket: WebSocket) -> Result<()> {
        let address = self.address(None).await?;
        let (host, runtime) = self.runtime(address).await?;
        if !matches!(runtime.as_ref(), RuntimeFace::Fleet(_)) {
            return Err(error("socket on non-session object"));
        }
        let binding = host.binding(&socket).map_err(error)?;
        socket_outcome(
            runtime
                .on_event(ObjectEvent::SocketClosed { binding })
                .await,
        )
    }
    async fn alarm(&self) -> Result<Response> {
        let address = self.address(None).await?;
        let (_, runtime) = self.runtime(address.clone()).await?;
        match self
            .deliver_alarm(
                &address,
                &runtime,
                #[cfg(feature = "conformance")]
                collaboration_central_core::event_trace::AlarmDelivery::Platform,
            )
            .await?
        {
            #[cfg(feature = "conformance")]
            AlarmOutcome::Dropped => Response::ok("alarm dropped"),
            AlarmOutcome::Applied => Response::ok("alarm applied"),
        }
    }
    /// The object's alarm entry: what the platform calls, and what a
    /// conformance `settleClock` injects through (labelled in S1).
    async fn deliver_alarm(
        &self,
        address: &collaboration_central_core::ObjectAddress,
        runtime: &RuntimeFace,
        #[cfg(feature = "conformance")]
        delivery: collaboration_central_core::event_trace::AlarmDelivery,
    ) -> Result<AlarmOutcome> {
        #[cfg(feature = "conformance")]
        if !self.alarm_delivery(address).await? {
            return Ok(AlarmOutcome::Dropped);
        }
        #[cfg(not(feature = "conformance"))]
        let _ = address;
        #[cfg(feature = "conformance")]
        let reply = runtime
            .on_event_with(
                ObjectEvent::Alarm,
                collaboration_central_core::event_trace::EventContext {
                    redelivery: None,
                    delivery,
                },
            )
            .await;
        #[cfg(not(feature = "conformance"))]
        let reply = runtime.on_event(ObjectEvent::Alarm).await;
        match reply {
            // Cloudflare redelivers a failed alarm event up to 6 times from 2 s.
            EventReply::AlarmRetry => Err(error("alarm recompute failed")),
            _ => Ok(AlarmOutcome::Applied),
        }
    }
}
/// What the object's alarm entry did with one delivery.
enum AlarmOutcome {
    /// `alarmDelivery` is off: dropped, with its `centralAlarmDropped` line.
    #[cfg(feature = "conformance")]
    Dropped,
    Applied,
}
/// A failed socket event errors the handler, which closes that connection, as before.
fn socket_outcome(reply: EventReply) -> Result<()> {
    match reply {
        EventReply::SocketFailed => Err(error("socket event failed")),
        _ => Ok(()),
    }
}
fn kernel<Face: collaboration_central_core::KernelFace>(
    face: Face,
    #[cfg(feature = "conformance")] observer: Arc<
        dyn collaboration_central_core::event_trace::KernelObserver,
    >,
) -> ObjectKernel<Face> {
    #[cfg(feature = "conformance")]
    {
        ObjectKernel::observed(face, observer)
    }
    #[cfg(not(feature = "conformance"))]
    {
        ObjectKernel::new(face)
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
