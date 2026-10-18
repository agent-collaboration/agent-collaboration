use crate::{
    object_context::{ObjectContext, SocketAttachment},
    object_rpc,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::CentralFrame;
use futures_util::future::LocalBoxFuture;
use std::collections::VecDeque;
struct DispatchReply {
    id: ObjectCallId,
    result: WireResult,
    carry: Option<DispatchCarry>,
}
trait CloudflareFace: EffectFace {
    fn invoke<'a>(
        context: &'a ObjectContext,
        call: Self::Outgoing,
    ) -> LocalBoxFuture<'a, worker::Result<Option<DispatchReply>>>;
}
impl CloudflareFace for SessionKind {
    fn invoke<'a>(
        context: &'a ObjectContext,
        call: Self::Outgoing,
    ) -> LocalBoxFuture<'a, worker::Result<Option<DispatchReply>>> {
        Box::pin(async move {
            match call {
                SessionDispatch::Registry(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result =
                        object_rpc::invoke::<SessionKind, RegistryKind>(context, call).await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: RegistryKind::result(result),
                        carry,
                    }))
                }
                SessionDispatch::Copy(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result = object_rpc::invoke::<SessionKind, CopyKind>(context, call).await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: CopyKind::result(result),
                        carry,
                    }))
                }
                SessionDispatch::Fleet(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result =
                        object_rpc::invoke::<SessionKind, FleetKind>(context, call).await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: FleetKind::result(result),
                        carry,
                    }))
                }
                SessionDispatch::Delivery(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result =
                        object_rpc::invoke::<SessionKind, DeliveryKind>(context, call).await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: DeliveryKind::result(result),
                        carry,
                    }))
                }
                SessionDispatch::Notice { installation, kind } => {
                    object_rpc::invoke::<SessionKind, DeliveryKind>(
                        context,
                        Dispatch {
                            call_id: ObjectCallId::new(uuid::Uuid::now_v7().to_string())
                                .map_err(|error| worker::Error::RustError(error.to_string()))?,
                            to: installation,
                            call: DeliveryCall::DeliverNotice(kind),
                            carry: None,
                        },
                    )
                    .await?;
                    Ok(None)
                }
            }
        })
    }
}
impl CloudflareFace for RegistryKind {
    fn invoke<'a>(
        context: &'a ObjectContext,
        call: Self::Outgoing,
    ) -> LocalBoxFuture<'a, worker::Result<Option<DispatchReply>>> {
        Box::pin(async move {
            match call {
                RegistryDispatch::Copy(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result =
                        object_rpc::invoke::<RegistryKind, CopyKind>(context, call).await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: CopyKind::result(result),
                        carry,
                    }))
                }
                RegistryDispatch::Fleet(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result =
                        object_rpc::invoke::<RegistryKind, FleetKind>(context, call).await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: FleetKind::result(result),
                        carry,
                    }))
                }
                RegistryDispatch::Delivery(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result =
                        object_rpc::invoke::<RegistryKind, DeliveryKind>(context, call).await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: DeliveryKind::result(result),
                        carry,
                    }))
                }
                RegistryDispatch::Notice { installation, kind } => {
                    object_rpc::invoke::<RegistryKind, DeliveryKind>(
                        context,
                        Dispatch {
                            call_id: ObjectCallId::new(uuid::Uuid::now_v7().to_string())
                                .map_err(|error| worker::Error::RustError(error.to_string()))?,
                            to: installation,
                            call: DeliveryCall::DeliverNotice(kind),
                            carry: None,
                        },
                    )
                    .await?;
                    Ok(None)
                }
            }
        })
    }
}
impl CloudflareFace for CopyKind {
    fn invoke<'a>(
        context: &'a ObjectContext,
        call: Self::Outgoing,
    ) -> LocalBoxFuture<'a, worker::Result<Option<DispatchReply>>> {
        Box::pin(async move {
            match call {
                CopyDispatch::Fleet(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result = object_rpc::invoke::<CopyKind, FleetKind>(context, call).await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: FleetKind::result(result),
                        carry,
                    }))
                }
            }
        })
    }
}
impl CloudflareFace for FleetKind {
    fn invoke<'a>(
        context: &'a ObjectContext,
        call: Self::Outgoing,
    ) -> LocalBoxFuture<'a, worker::Result<Option<DispatchReply>>> {
        Box::pin(async move {
            match call {
                FleetDispatch::Delivery(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result =
                        object_rpc::invoke::<FleetKind, DeliveryKind>(context, call).await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: DeliveryKind::result(result),
                        carry,
                    }))
                }
            }
        })
    }
}
impl CloudflareFace for DeliveryKind {
    fn invoke<'a>(
        _context: &'a ObjectContext,
        call: NoDispatch,
    ) -> LocalBoxFuture<'a, worker::Result<Option<DispatchReply>>> {
        match call {}
    }
}
impl ObjectContext {
    pub fn carry<'a>(
        &'a self,
        first: ObjectEffects,
    ) -> LocalBoxFuture<'a, worker::Result<Vec<WireEffect>>> {
        match first {
            ObjectEffects::Session(values) => self.carry_face::<SessionKind>(values),
            ObjectEffects::Registry(values) => self.carry_face::<RegistryKind>(values),
            ObjectEffects::Copy(values) => self.carry_face::<CopyKind>(values),
            ObjectEffects::Fleet(values) => self.carry_face::<FleetKind>(values),
            ObjectEffects::Delivery(values) => self.carry_face::<DeliveryKind>(values),
        }
    }
    fn carry_face<'a, K: CloudflareFace + 'a>(
        &'a self,
        first: Vec<K::Effect>,
    ) -> LocalBoxFuture<'a, worker::Result<Vec<WireEffect>>> {
        Box::pin(async move {
            if self.face != K::KIND {
                return Err(worker::Error::RustError(
                    "effect face differs from context".into(),
                ));
            }
            let mut trace = first.iter().map(K::trace).collect::<Vec<_>>();
            let mut pending = VecDeque::from(first);
            while let Some(effect) = pending.pop_front() {
                match K::action(effect) {
                    EffectAction::Dispatch(call) => {
                        let metadata = call.metadata();
                        #[cfg(feature = "conformance")]
                        if let Some(metadata) = &metadata {
                            crate::conformance_control::before_dispatch(self, &metadata.call)
                                .await?;
                        }
                        let reply = match K::invoke(self, call).await {
                            Ok(Some(reply)) => reply,
                            Ok(None) => continue,
                            Err(_)
                                if metadata.as_ref().is_some_and(|item| {
                                    matches!(
                                        &item.call,
                                        WireCall::Fleet(
                                            FleetCall::DirectoryUpdate(_)
                                                | FleetCall::ReleaseClaim { .. }
                                                | FleetCall::AdvanceLeaseIncarnation { .. }
                                        )
                                    )
                                }) =>
                            {
                                continue;
                            }
                            Err(_)
                                if metadata.as_ref().is_some_and(|item| is_forward(&item.call)) =>
                            {
                                let item = metadata.ok_or_else(|| {
                                    worker::Error::RustError(
                                        "missing forward dispatch metadata".into(),
                                    )
                                })?;
                                let next = K::unpack(
                                    self.step(WireInput::CallFailed {
                                        source: K::KIND,
                                        call_id: item.call_id,
                                        carry: item.carry,
                                    })
                                    .await?,
                                )
                                .map_err(core_error)?;
                                trace.extend(next.iter().map(K::trace));
                                pending.extend(next);
                                continue;
                            }
                            Err(error) => return Err(error),
                        };
                        #[cfg(feature = "conformance")]
                        if metadata.as_ref().is_some_and(|item| {
                            matches!(&item.call, WireCall::Copy(CopyCall::Fence { .. }))
                        }) && crate::conformance_control::consume(self, "stop_after_fence")
                            .await?
                        {
                            return Err(worker::Error::RustError(
                                "conformance interruption after committed fence".into(),
                            ));
                        }
                        let next = K::unpack(
                            self.step(WireInput::CallResult {
                                source: K::KIND,
                                call_id: reply.id,
                                result: Box::new(reply.result),
                                carry: reply.carry,
                            })
                            .await?,
                        )
                        .map_err(core_error)?;
                        trace.extend(next.iter().map(K::trace));
                        pending.extend(next);
                    }
                    EffectAction::Send { to, frame } => {
                        #[cfg(feature = "conformance")]
                        if matches!(&frame, CentralFrame::Answer { .. })
                            && crate::conformance_control::consume(self, "drop_answer").await?
                        {
                            crate::conformance_control::signal(self, "answer_dropped");
                            continue;
                        }
                        for socket in &self.sockets {
                            if let Some(attachment) =
                                socket.deserialize_attachment::<SocketAttachment>()?
                                && attachment.connection_id == to
                            {
                                socket.send_with_bytes(serde_json::to_vec(&frame)?)?;
                                if matches!(frame, CentralFrame::Closed { .. }) {
                                    socket.close(Some(1000), Some("core closed connection"))?;
                                }
                                break;
                            }
                        }
                    }
                    EffectAction::SetAlarm { at } => self.set_face_alarm(K::KIND, at).await?,
                    EffectAction::Return { .. } => {}
                }
            }
            Ok(trace)
        })
    }
    pub async fn run(&self, input: WireInput) -> worker::Result<Vec<WireEffect>> {
        let first = self.step(input).await?;
        self.carry(first).await
    }
    pub async fn answer_call(&self, input: WireInput) -> worker::Result<WireResult> {
        let effects = self.run(input.clone()).await?;
        validate_call_returns(&input, &effects).map_err(core_error)?;
        effects
            .into_iter()
            .find_map(|effect| match effect {
                WireEffect::Return { result, .. } => Some(result),
                _ => None,
            })
            .ok_or_else(|| worker::Error::RustError("missing object Return".into()))
    }
}
fn core_error(error: CentralError) -> worker::Error {
    worker::Error::RustError(error.to_string())
}
fn is_forward(call: &WireCall) -> bool {
    matches!(
        call,
        WireCall::Registry(RegistryCall::ForwardRoute { .. })
            | WireCall::Delivery(
                DeliveryCall::DeliverForward { .. } | DeliveryCall::ForwardAnswered { .. }
            )
    )
}
