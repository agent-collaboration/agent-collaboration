use crate::{
    object_context::{ObjectContext, SocketAttachment},
    object_rpc::ObjectRequest,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use std::{cell::RefCell, rc::Rc};
use worker::*;
#[derive(Clone, Copy)]
enum PhysicalKind {
    Connection,
    Copy,
    Registry,
    Fleet,
}
impl PhysicalKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Connection => "connection",
            Self::Copy => "copy",
            Self::Registry => "registry",
            Self::Fleet => "fleet",
        }
    }
    fn matches(self, address: &ObjectAddress) -> bool {
        matches!(
            (self, address),
            (Self::Connection, ObjectAddress::Connection(_))
                | (Self::Copy, ObjectAddress::Copy(_))
                | (Self::Registry, ObjectAddress::Registry)
                | (Self::Fleet, ObjectAddress::Fleet)
        )
    }
}
pub(crate) struct ShellActor {
    instance_id: uuid::Uuid,
    pub(crate) state: State,
    pub(crate) env: Env,
    kind: PhysicalKind,
    pub(crate) runtime: RefCell<Option<(ObjectAddress, Rc<Storage>)>>,
    #[cfg(feature = "conformance")]
    pub(crate) signals: Rc<crate::conformance_control::SignalSlots>,
}
impl ShellActor {
    fn new(state: State, env: Env, kind: PhysicalKind) -> Self {
        let instance_id = uuid::Uuid::now_v7();
        // One line per constructed instance: a woken hibernated object logs a
        // new label inside the invocation that woke it. Kind and a random
        // label only; never the object's address.
        console_log!(
            "central object instance: kind={} instance={}",
            kind.label(),
            instance_id
        );
        Self {
            instance_id,
            state,
            env,
            kind,
            runtime: RefCell::new(None),
            #[cfg(feature = "conformance")]
            signals: Rc::new(crate::conformance_control::SignalSlots::default()),
        }
    }
    pub(crate) async fn context(
        &self,
        requested: Option<ObjectAddress>,
        face: CallerKind,
    ) -> Result<ObjectContext> {
        let cached = self.runtime.borrow().clone();
        let (address, storage) = if let Some(cached) = cached {
            cached
        } else {
            let storage = Rc::new(self.state.storage());
            let stored = storage.get::<ObjectAddress>("_cf_actor_address").await?;
            let address = match (stored, requested.clone()) {
                (Some(stored), Some(expected)) if stored != expected => {
                    return Err(Error::RustError("object address mismatch".into()));
                }
                (Some(stored), _) => stored,
                (None, Some(address)) => {
                    let saved = address.clone();
                    storage
                        .transaction(move |transaction| async move {
                            transaction.put("_cf_actor_address", saved).await
                        })
                        .await?;
                    address
                }
                _ => return Err(Error::RustError("object address missing".into())),
            };
            if !self.kind.matches(&address) {
                return Err(Error::RustError("wrong object kind".into()));
            }
            let cached = (address, storage);
            *self.runtime.borrow_mut() = Some(cached.clone());
            cached
        };
        if let Some(expected) = requested
            && expected != address
        {
            return Err(Error::RustError("object address mismatch".into()));
        }
        let context = ObjectContext {
            registry_run: if matches!(address, ObjectAddress::Registry) {
                Some(
                    RegistryRunId::new(self.instance_id.to_string())
                        .map_err(|error| Error::RustError(error.to_string()))?,
                )
            } else {
                None
            },
            address,
            face,
            #[cfg(feature = "conformance")]
            signals: self.signals.clone(),
            storage,
            env: self.env.clone(),
            sockets: self.state.get_websockets(),
        };
        Ok(context)
    }
    async fn fetch(&self, mut request: Request) -> Result<Response> {
        let path = request.path();
        if path == "/v1/session" {
            let credential: CredentialStatus = serde_json::from_str(
                &request
                    .headers()
                    .get("x-central-credential")?
                    .ok_or_else(|| Error::RustError("verified identity missing".into()))?,
            )?;
            let CredentialStatus::Machine { installation, .. } = &credential else {
                return Response::error("machine required", 401);
            };
            let address = ObjectAddress::Connection(installation.clone());
            let context = self.context(Some(address), CallerKind::Session).await?;
            #[cfg(feature = "conformance")]
            if let Some(clock) = request.headers().get("x-central-test-clock")? {
                context
                    .storage
                    .put(
                        "_cf_test_clock",
                        serde_json::from_str::<RegistryTime>(&clock)?,
                    )
                    .await?;
            }
            #[cfg(not(feature = "conformance"))]
            let _ = context;
            let pair = WebSocketPair::new()?;
            pair.server.serialize_attachment(SocketAttachment {
                installation: installation.clone(),
                connection_id: ConnectionId::new(uuid::Uuid::now_v7().to_string())
                    .map_err(|error| Error::RustError(error.to_string()))?,
                session_id: None,
                credential,
            })?;
            self.state.accept_web_socket(&pair.server);
            return Response::from_websocket(pair.client);
        }
        #[cfg(feature = "conformance")]
        if path.starts_with("/test/") {
            return crate::conformance_control::object_control(self, &path, &mut request).await;
        }
        if path != "/invoke" {
            return Response::error("not found", 404);
        }
        let request: ObjectRequest = serde_json::from_str(&request.text().await?)?;
        if let Err(refusal) = crate::object_auth::authorized(
            request.caller_kind,
            &request.caller,
            &request.address,
            &request.input,
        ) {
            return Ok(Response::from_json(&refusal)?.with_status(403));
        }
        let face = request.input.face();
        let context = self.context(Some(request.address), face).await?;
        #[cfg(feature = "conformance")]
        if let Some(clock) = request.test_clock {
            context.storage.put("_cf_test_clock", clock).await?;
        }
        match context.answer_call(request.input).await {
            Ok(result) => Response::from_json(&result),
            Err(error) => {
                console_error!("central invocation: {}", error);
                Response::error("object invocation failed", 500)
            }
        }
    }
    async fn websocket_message(
        &self,
        socket: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        let mut attachment = socket
            .deserialize_attachment::<SocketAttachment>()?
            .ok_or_else(|| Error::RustError("socket attachment missing".into()))?;
        let bytes = match message {
            WebSocketIncomingMessage::Binary(bytes) => bytes,
            WebSocketIncomingMessage::String(text) => text.into_bytes(),
        };
        let frame = decode_machine_frame(&bytes);
        let context = self
            .context(
                Some(ObjectAddress::Connection(attachment.installation.clone())),
                CallerKind::Session,
            )
            .await?;
        let input = match frame {
            Err(failure) => WireInput::Session(SessionCall::DecodeFailure {
                connection: attachment.connection_id.clone(),
                failure,
            }),
            Ok(MachineFrame::Hello {
                installation,
                session_id,
                schema_support,
            }) => {
                attachment.session_id = Some(session_id.clone());
                socket.serialize_attachment(&attachment)?;
                WireInput::Session(SessionCall::Hello {
                    connection: attachment.connection_id.clone(),
                    credential: attachment.credential.clone(),
                    installation,
                    session_id,
                    schema_support,
                })
            }
            Ok(MachineFrame::ForwardAnswer { forward_id, reply }) => {
                WireInput::Session(SessionCall::ForwardAnswer {
                    connection: attachment.connection_id.clone(),
                    forward_id,
                    reply,
                })
            }
            Ok(MachineFrame::Request { frame_id, body }) => {
                WireInput::Session(SessionCall::Frame {
                    connection: attachment.connection_id.clone(),
                    frame_id,
                    request: body,
                })
            }
        };
        let effects = context.step(input).await?;
        self.state.wait_until(async move {
            if let Err(error) = context.carry(effects).await {
                console_error!("central socket effect: {}", error);
                let _ = socket.close(Some(1011), Some("unknown request outcome"));
            }
        });
        Ok(())
    }
    async fn disconnected(&self, socket: WebSocket) -> Result<()> {
        if let Some(attachment) = socket.deserialize_attachment::<SocketAttachment>()? {
            let context = self
                .context(
                    Some(ObjectAddress::Connection(attachment.installation)),
                    CallerKind::Session,
                )
                .await?;
            context
                .run(WireInput::Session(SessionCall::Disconnected {
                    connection: attachment.connection_id,
                }))
                .await?;
        }
        Ok(())
    }
    async fn alarm(&self) -> Result<Response> {
        let context = self
            .context(
                None,
                match self.kind {
                    PhysicalKind::Connection => CallerKind::Session,
                    PhysicalKind::Copy => CallerKind::Copy,
                    PhysicalKind::Registry => CallerKind::Registry,
                    PhysicalKind::Fleet => CallerKind::Fleet,
                },
            )
            .await?;
        for face in context.take_due_alarms().await? {
            let owned = self.context(None, face).await?;
            owned.run(WireInput::Alarm { face }).await?;
        }
        context.refresh_alarm().await?;
        Response::ok("alarm applied")
    }
}
macro_rules! shell {
    ($name:ident,$kind:ident) => {
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
                _code: usize,
                _reason: String,
                _clean: bool,
            ) -> Result<()> {
                self.actor.disconnected(socket).await
            }
            async fn websocket_error(&self, socket: WebSocket, _error: Error) -> Result<()> {
                self.actor.disconnected(socket).await
            }
        }
    };
}
shell!(ConnectionObject, Connection);
shell!(ProjectCopyObject, Copy);
shell!(LeaseRegistryObject, Registry);
shell!(FleetObject, Fleet);
