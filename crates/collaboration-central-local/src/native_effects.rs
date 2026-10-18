use crate::{LocalResult, SessionRuntime};
use collaboration_central_core::*;
use collaboration_replication_protocol::CentralFrame;
use futures_util::future::BoxFuture;
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
pub(crate) struct DispatchReply {
    pub id: ObjectCallId,
    pub result: WireResult,
    pub carry: Option<DispatchCarry>,
}
pub(crate) trait NativeFace: EffectFace {
    fn invoke<'a>(
        runtime: &'a SessionRuntime,
        from: ObjectAddress,
        call: Self::Outgoing,
    ) -> BoxFuture<'a, LocalResult<Option<DispatchReply>>>;
}
impl NativeFace for SessionKind {
    fn invoke<'a>(
        runtime: &'a SessionRuntime,
        from: ObjectAddress,
        call: Self::Outgoing,
    ) -> BoxFuture<'a, LocalResult<Option<DispatchReply>>> {
        Box::pin(async move {
            match call {
                SessionDispatch::Registry(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result = runtime
                        .invoke_typed::<SessionKind, RegistryKind>(from, call)
                        .await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: RegistryKind::result(result),
                        carry,
                    }))
                }
                SessionDispatch::Copy(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result = runtime
                        .invoke_typed::<SessionKind, CopyKind>(from, call)
                        .await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: CopyKind::result(result),
                        carry,
                    }))
                }
                SessionDispatch::Fleet(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result = runtime
                        .invoke_typed::<SessionKind, FleetKind>(from, call)
                        .await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: FleetKind::result(result),
                        carry,
                    }))
                }
                SessionDispatch::Delivery(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result = runtime
                        .invoke_typed::<SessionKind, DeliveryKind>(from, call)
                        .await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: DeliveryKind::result(result),
                        carry,
                    }))
                }
                SessionDispatch::Notice { installation, kind } => {
                    runtime
                        .invoke_typed::<SessionKind, DeliveryKind>(
                            from,
                            Dispatch {
                                call_id: ObjectCallId::new(uuid::Uuid::now_v7().to_string())?,
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
impl NativeFace for RegistryKind {
    fn invoke<'a>(
        runtime: &'a SessionRuntime,
        from: ObjectAddress,
        call: Self::Outgoing,
    ) -> BoxFuture<'a, LocalResult<Option<DispatchReply>>> {
        Box::pin(async move {
            match call {
                RegistryDispatch::Copy(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result = runtime
                        .invoke_typed::<RegistryKind, CopyKind>(from, call)
                        .await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: CopyKind::result(result),
                        carry,
                    }))
                }
                RegistryDispatch::Fleet(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result = runtime
                        .invoke_typed::<RegistryKind, FleetKind>(from, call)
                        .await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: FleetKind::result(result),
                        carry,
                    }))
                }
                RegistryDispatch::Delivery(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result = runtime
                        .invoke_typed::<RegistryKind, DeliveryKind>(from, call)
                        .await?;
                    Ok(Some(DispatchReply {
                        id,
                        result: DeliveryKind::result(result),
                        carry,
                    }))
                }
                RegistryDispatch::Notice { installation, kind } => {
                    runtime
                        .invoke_typed::<RegistryKind, DeliveryKind>(
                            from,
                            Dispatch {
                                call_id: ObjectCallId::new(uuid::Uuid::now_v7().to_string())?,
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
impl NativeFace for CopyKind {
    fn invoke<'a>(
        runtime: &'a SessionRuntime,
        from: ObjectAddress,
        call: Self::Outgoing,
    ) -> BoxFuture<'a, LocalResult<Option<DispatchReply>>> {
        Box::pin(async move {
            match call {
                CopyDispatch::Fleet(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result = runtime
                        .invoke_typed::<CopyKind, FleetKind>(from, call)
                        .await?;
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
impl NativeFace for FleetKind {
    fn invoke<'a>(
        runtime: &'a SessionRuntime,
        from: ObjectAddress,
        call: Self::Outgoing,
    ) -> BoxFuture<'a, LocalResult<Option<DispatchReply>>> {
        Box::pin(async move {
            match call {
                FleetDispatch::Delivery(call) => {
                    let id = call.call_id.clone();
                    let carry = call.carry.clone();
                    let result = runtime
                        .invoke_typed::<FleetKind, DeliveryKind>(from, call)
                        .await?;
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
impl NativeFace for DeliveryKind {
    fn invoke<'a>(
        _runtime: &'a SessionRuntime,
        _from: ObjectAddress,
        call: NoDispatch,
    ) -> BoxFuture<'a, LocalResult<Option<DispatchReply>>> {
        match call {}
    }
}
impl SessionRuntime {
    pub(crate) fn carry<'a>(
        &'a self,
        address: ObjectAddress,
        first: ObjectEffects,
    ) -> BoxFuture<'a, LocalResult<Vec<WireEffect>>> {
        match first {
            ObjectEffects::Session(values) => self.carry_face::<SessionKind>(address, values),
            ObjectEffects::Registry(values) => self.carry_face::<RegistryKind>(address, values),
            ObjectEffects::Copy(values) => self.carry_face::<CopyKind>(address, values),
            ObjectEffects::Fleet(values) => self.carry_face::<FleetKind>(address, values),
            ObjectEffects::Delivery(values) => self.carry_face::<DeliveryKind>(address, values),
        }
    }
    fn carry_face<'a, K: NativeFace + Send + 'a>(
        &'a self,
        address: ObjectAddress,
        first: Vec<K::Effect>,
    ) -> BoxFuture<'a, LocalResult<Vec<WireEffect>>>
    where
        K::Effect: Send,
        K::Outgoing: Send,
        K::Result: Send,
    {
        Box::pin(async move {
            let mut trace = first.iter().map(K::trace).collect::<Vec<_>>();
            let mut pending = VecDeque::from(first);
            while let Some(effect) = pending.pop_front() {
                match K::action(effect) {
                    EffectAction::Dispatch(call) => {
                        let metadata = call.metadata();
                        #[cfg(feature = "conformance")]
                        if let Some(metadata) = &metadata {
                            self.before_dispatch(&address, K::KIND, &metadata.call)
                                .await?;
                        }
                        let result = match K::invoke(self, address.clone(), call).await {
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
                                let item = metadata.ok_or("missing forward dispatch metadata")?;
                                let next = K::unpack(
                                    self.actors
                                        .step(
                                            address.clone(),
                                            WireInput::CallFailed {
                                                source: K::KIND,
                                                call_id: item.call_id,
                                                carry: item.carry,
                                            },
                                            self.now(),
                                        )
                                        .await?,
                                )?;
                                trace.extend(next.iter().map(K::trace));
                                pending.extend(next);
                                continue;
                            }
                            Err(error) => return Err(error),
                        };
                        #[cfg(feature = "conformance")]
                        if metadata.as_ref().is_some_and(|item| {
                            matches!(&item.call, WireCall::Copy(CopyCall::Fence { .. }))
                        }) && self.stop_after_fence.swap(false, Ordering::SeqCst)
                        {
                            return Err("conformance interruption after committed fence".into());
                        }
                        let next = K::unpack(
                            self.actors
                                .step(
                                    address.clone(),
                                    WireInput::CallResult {
                                        source: K::KIND,
                                        call_id: result.id,
                                        result: Box::new(result.result),
                                        carry: result.carry,
                                    },
                                    self.now(),
                                )
                                .await?,
                        )?;
                        trace.extend(next.iter().map(K::trace));
                        pending.extend(next);
                    }
                    EffectAction::Send { to, frame } => {
                        #[cfg(feature = "conformance")]
                        if K::KIND == CallerKind::Session
                            && matches!(&frame, CentralFrame::Answer { .. })
                            && let ObjectAddress::Connection(installation) = &address
                        {
                            let faults = self.faults.session(installation).await;
                            if faults.drop_next_answer.swap(false, Ordering::SeqCst) {
                                faults.answer_dropped.notify_one();
                                continue;
                            }
                        }
                        if let Some(writer) = self.sockets.lock().await.get(to.as_str()).cloned() {
                            let _ = writer.send(frame);
                        }
                    }
                    EffectAction::SetAlarm { .. } => self.refresh_alarm(address.clone()).await?,
                    EffectAction::Return { .. } => {}
                }
            }
            Ok(trace)
        })
    }
    #[cfg(feature = "conformance")]
    async fn before_dispatch(
        &self,
        address: &ObjectAddress,
        face: CallerKind,
        call: &WireCall,
    ) -> LocalResult<()> {
        if matches!(call, WireCall::Copy(CopyCall::Fence { .. }))
            && self.faults.hold_next_fence.swap(false, Ordering::SeqCst)
        {
            self.faults.fence_held.notify_one();
            tokio::select! {_=self.stop.cancelled()=>return Err("server shutdown".into()),_=self.faults.resume_fence.notified()=>{}}
        }
        if face != CallerKind::Session {
            return Ok(());
        }
        let ObjectAddress::Connection(installation) = address else {
            return Err("session fault address".into());
        };
        let faults = self.faults.session(installation).await;
        if matches!(
            call,
            WireCall::Delivery(DeliveryCall::DeliverForward { .. })
        ) && faults.hold_forward_delivery.swap(false, Ordering::SeqCst)
        {
            faults.forward_delivery_held.notify_one();
            tokio::select! {_=self.stop.cancelled()=>return Err("server shutdown".into()),_=faults.resume_forward_delivery.notified()=>{}}
        }
        if faults.hold_next_dispatch.swap(false, Ordering::SeqCst) {
            *faults.held_dispatch.lock().await = Some(call.clone());
            faults.dispatch_held.notify_one();
            tokio::select! {_=self.stop.cancelled()=>return Err("server shutdown".into()),_=faults.resume_dispatch.notified()=>{}}
        }
        Ok(())
    }
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
