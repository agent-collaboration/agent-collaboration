//! One shared-task Host; runtimes supply byte transport, time, IDs and wakes.

use crate::registry_peer_lookup::PeerDirectory;
use crate::{
    CallerContext, DeliveryKind, ObjectAddress, RegistryKind, SerializedObjectCall, SessionKind,
    object_call_envelope::{encode_refusal, encode_shared_call},
};
use durable_steps::{CallFailure, CallId, CallTarget, Host, NotSentReason};
use fleet_sync_protocol::{PeerId, ReceiverId};
use message_board::ServiceId;
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde::de::DeserializeOwned;
use std::future::Future;

mod session_routes;

pub trait PlatformIo: Clock + IdSource + WakeScheduler + Send + Sync + 'static {
    fn reply_deadline(&self, remaining: std::time::Duration) -> impl Future<Output = ()> + Send;
    fn object_call(
        &self,
        destination: ObjectAddress,
        body: Vec<u8>,
    ) -> impl Future<Output = Result<Vec<u8>, CallFailure>> + Send;
}

pub(crate) type LocalFleetCalls = std::sync::Arc<
    dyn Fn(Vec<u8>) -> durable_steps::BoxFuture<'static, Result<Vec<u8>, CallFailure>>
        + Send
        + Sync,
>;

pub struct CoreHost<Io: PlatformIo> {
    local_calls: std::sync::Arc<std::sync::OnceLock<LocalFleetCalls>>,
    io: std::sync::Arc<Io>,
    caller: SharedTaskCaller,
    authoritative_receiver: ReceiverId,
}

enum SharedTaskCaller {
    Registry {
        caller: CallerContext<RegistryKind>,
        peer_directory: PeerDirectory,
    },
    StoreOnly,
    Session(CallerContext<SessionKind>),
    Delivery,
}

impl<Io: PlatformIo> CoreHost<Io> {
    pub(crate) fn registry(
        io: std::sync::Arc<Io>,
        authoritative_receiver: ReceiverId,
        peer_directory: PeerDirectory,
    ) -> Self {
        Self {
            io,
            authoritative_receiver,
            caller: SharedTaskCaller::Registry {
                caller: CallerContext::for_object(()),
                peer_directory,
            },
            local_calls: Default::default(),
        }
    }

    pub fn for_store(io: Io, authoritative_receiver: ReceiverId) -> Self {
        Self {
            io: std::sync::Arc::new(io),
            authoritative_receiver,
            caller: SharedTaskCaller::StoreOnly,
            local_calls: Default::default(),
        }
    }

    pub(crate) fn delivery(&self) -> Self {
        Self {
            io: self.io.clone(),
            caller: SharedTaskCaller::Delivery,
            authoritative_receiver: self.authoritative_receiver,
            local_calls: self.local_calls.clone(),
        }
    }

    pub(crate) fn session_for(&self, installation: ServiceId) -> Self {
        Self {
            io: self.io.clone(),
            caller: SharedTaskCaller::Session(CallerContext::for_machine(installation)),
            authoritative_receiver: self.authoritative_receiver,
            local_calls: self.local_calls.clone(),
        }
    }
    pub(crate) fn authoritative_receiver(&self) -> ReceiverId {
        self.authoritative_receiver
    }

    pub(crate) fn install_local_calls(
        &self,
        calls: LocalFleetCalls,
    ) -> Result<(), durable_steps::StoreError> {
        self.local_calls.set(calls).map_err(|_| {
            durable_steps::StoreError::backend("Fleet local dispatcher already installed")
        })
    }

    async fn classify<Call: CallTarget>(
        &self,
        target: Call,
        request: Call::Request,
        identity: CallId,
    ) -> Result<SerializedObjectCall, CallFailure> {
        let target = serde_json::to_value(target).map_err(encode_refusal)?;
        let request = serde_json::to_value(request).map_err(encode_refusal)?;
        match &self.caller {
            SharedTaskCaller::Session(caller) => {
                session_routes::classify_session(Call::KIND, caller, target, request, identity)
            }
            SharedTaskCaller::StoreOnly | SharedTaskCaller::Delivery => Err(no_route()),
            SharedTaskCaller::Registry {
                caller,
                peer_directory,
            } => {
                if Call::KIND == fleet_sync_session::SchemaSupportRegistry::KIND {
                    let peer: PeerId =
                        decode::<fleet_sync_protocol::ReportSchemaSupport>(request.clone())?.peer;
                    let source =
                        CallerContext::<SessionKind>::for_machine(peer_directory(peer).await?);
                    return session_routes::classify_session(
                        Call::KIND,
                        &source,
                        target,
                        request,
                        identity,
                    );
                }
                match Call::KIND {
                    crate::machine_enrollment::EndPeerSessions::KIND => {
                        let target: crate::machine_enrollment::EndPeerSessions = decode(target)?;
                        encode_shared_call::<
                            RegistryKind,
                            DeliveryKind,
                            crate::machine_enrollment::EndPeerSessions,
                        >(caller, (), target, decode(request)?, identity)
                    }
                    _ => Err(no_route()),
                }
            }
        }
    }
    async fn transmit(&self, call: SerializedObjectCall) -> Result<Vec<u8>, CallFailure> {
        if call.destination == ObjectAddress::Fleet
            && let Some(local) = self.local_calls.get()
        {
            return local(call.bytes).await;
        }
        self.io.object_call(call.destination, call.bytes).await
    }
}

impl<Io: PlatformIo> Clock for CoreHost<Io> {
    fn now(&self) -> SyncTime {
        self.io.now()
    }
}
impl<Io: PlatformIo> IdSource for CoreHost<Io> {
    fn new_id(&self) -> durable_steps::Uuid {
        self.io.new_id()
    }
}
impl<Io: PlatformIo> WakeScheduler for CoreHost<Io> {
    async fn wake_at(&self, at: SyncTime) -> Result<(), WakeFailure> {
        self.io.wake_at(at).await
    }
}
impl<Io: PlatformIo> Host for CoreHost<Io> {
    async fn dispatch<Call: CallTarget>(
        &self,
        target: Call,
        request: Call::Request,
        identity: CallId,
    ) -> Result<Call::Answer, CallFailure> {
        let call = self.classify(target, request, identity).await?;
        let answer = self.transmit(call).await?;
        serde_json::from_slice(&answer).map_err(|_| CallFailure::AnswerLost)
    }
}

fn decode<Value: DeserializeOwned>(value: serde_json::Value) -> Result<Value, CallFailure> {
    serde_json::from_value(value).map_err(encode_refusal)
}

fn no_route() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
