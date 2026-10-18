//! Port-side execution retains the originating face and its typed outgoing union.
use crate::*;
use collaboration_replication_protocol::CentralFrame;
#[derive(Clone, Debug, PartialEq)]
pub enum NoDispatch {}
pub enum EffectAction<Outgoing, Reply> {
    Send {
        to: ConnectionId,
        frame: CentralFrame,
    },
    Dispatch(Outgoing),
    Return {
        call_id: ObjectCallId,
        result: Reply,
    },
    SetAlarm {
        at: RegistryTime,
    },
}
pub trait EffectFace: CoreKind {
    fn trace(effect: &Self::Effect) -> WireEffect;
    type Outgoing: Clone + DispatchView;
    fn action(effect: Self::Effect) -> EffectAction<Self::Outgoing, Self::Result>;
}
#[derive(Clone)]
pub struct DispatchMetadata {
    pub call_id: ObjectCallId,
    pub call: WireCall,
    pub carry: Option<DispatchCarry>,
}
pub trait DispatchView {
    fn metadata(&self) -> Option<DispatchMetadata>;
}
impl DispatchView for NoDispatch {
    fn metadata(&self) -> Option<DispatchMetadata> {
        match *self {}
    }
}
pub trait WireKind: ObjectKind {
    fn call(value: Self::Call) -> WireCall;
    fn result(value: Self::Result) -> WireResult;
    fn decode_result(value: WireResult) -> Result<Self::Result, CentralError>;
}
impl EffectFace for SessionKind {
    fn trace(effect: &Self::Effect) -> WireEffect {
        effect.clone().into()
    }
    type Outgoing = SessionDispatch;
    fn action(effect: Self::Effect) -> EffectAction<Self::Outgoing, Self::Result> {
        match effect {
            SessionEffect::Send { to, frame } => EffectAction::Send { to, frame },
            SessionEffect::Dispatch(call) => EffectAction::Dispatch(call),
            SessionEffect::Return { call_id, result } => EffectAction::Return { call_id, result },
            SessionEffect::SetAlarm { at } => EffectAction::SetAlarm { at },
        }
    }
}
impl DispatchView for SessionDispatch {
    fn metadata(&self) -> Option<DispatchMetadata> {
        match self {
            Self::Registry(call) => Some(DispatchMetadata {
                call_id: call.call_id.clone(),
                call: RegistryKind::call(call.call.clone()),
                carry: call.carry.clone(),
            }),
            Self::Copy(call) => Some(DispatchMetadata {
                call_id: call.call_id.clone(),
                call: CopyKind::call(call.call.clone()),
                carry: call.carry.clone(),
            }),
            Self::Fleet(call) => Some(DispatchMetadata {
                call_id: call.call_id.clone(),
                call: FleetKind::call(call.call.clone()),
                carry: call.carry.clone(),
            }),
            Self::Delivery(call) => Some(DispatchMetadata {
                call_id: call.call_id.clone(),
                call: DeliveryKind::call(call.call.clone()),
                carry: call.carry.clone(),
            }),
            Self::Notice { .. } => None,
        }
    }
}
impl EffectFace for RegistryKind {
    fn trace(effect: &Self::Effect) -> WireEffect {
        effect.clone().into()
    }
    type Outgoing = RegistryDispatch;
    fn action(effect: Self::Effect) -> EffectAction<Self::Outgoing, Self::Result> {
        match effect {
            RegistryEffect::Send { to, frame } => EffectAction::Send { to, frame },
            RegistryEffect::Dispatch(call) => EffectAction::Dispatch(call),
            RegistryEffect::Return { call_id, result } => EffectAction::Return { call_id, result },
            RegistryEffect::SetAlarm { at } => EffectAction::SetAlarm { at },
        }
    }
}
impl DispatchView for RegistryDispatch {
    fn metadata(&self) -> Option<DispatchMetadata> {
        match self {
            Self::Copy(call) => Some(DispatchMetadata {
                call_id: call.call_id.clone(),
                call: CopyKind::call(call.call.clone()),
                carry: call.carry.clone(),
            }),
            Self::Fleet(call) => Some(DispatchMetadata {
                call_id: call.call_id.clone(),
                call: FleetKind::call(call.call.clone()),
                carry: call.carry.clone(),
            }),
            Self::Delivery(call) => Some(DispatchMetadata {
                call_id: call.call_id.clone(),
                call: DeliveryKind::call(call.call.clone()),
                carry: call.carry.clone(),
            }),
            Self::Notice { .. } => None,
        }
    }
}
impl EffectFace for CopyKind {
    fn trace(effect: &Self::Effect) -> WireEffect {
        effect.clone().into()
    }
    type Outgoing = CopyDispatch;
    fn action(effect: Self::Effect) -> EffectAction<Self::Outgoing, Self::Result> {
        match effect {
            CopyEffect::Send { to, frame } => EffectAction::Send { to, frame },
            CopyEffect::Dispatch(call) => EffectAction::Dispatch(call),
            CopyEffect::Return { call_id, result } => EffectAction::Return { call_id, result },
            CopyEffect::SetAlarm { at } => EffectAction::SetAlarm { at },
        }
    }
}
impl DispatchView for CopyDispatch {
    fn metadata(&self) -> Option<DispatchMetadata> {
        match self {
            Self::Fleet(call) => Some(DispatchMetadata {
                call_id: call.call_id.clone(),
                call: FleetKind::call(call.call.clone()),
                carry: call.carry.clone(),
            }),
        }
    }
}
impl EffectFace for FleetKind {
    fn trace(effect: &Self::Effect) -> WireEffect {
        effect.clone().into()
    }
    type Outgoing = FleetDispatch;
    fn action(effect: Self::Effect) -> EffectAction<Self::Outgoing, Self::Result> {
        match effect {
            FleetEffect::Send { to, frame } => EffectAction::Send { to, frame },
            FleetEffect::Dispatch(call) => EffectAction::Dispatch(call),
            FleetEffect::Return { call_id, result } => EffectAction::Return { call_id, result },
            FleetEffect::SetAlarm { at } => EffectAction::SetAlarm { at },
        }
    }
}
impl DispatchView for FleetDispatch {
    fn metadata(&self) -> Option<DispatchMetadata> {
        match self {
            Self::Delivery(call) => Some(DispatchMetadata {
                call_id: call.call_id.clone(),
                call: DeliveryKind::call(call.call.clone()),
                carry: call.carry.clone(),
            }),
        }
    }
}
impl EffectFace for DeliveryKind {
    fn trace(effect: &Self::Effect) -> WireEffect {
        effect.clone().into()
    }
    type Outgoing = NoDispatch;
    fn action(effect: Self::Effect) -> EffectAction<Self::Outgoing, Self::Result> {
        match effect {
            DeliveryEffect::Send { to, frame } => EffectAction::Send { to, frame },
            DeliveryEffect::Return { call_id, result } => EffectAction::Return { call_id, result },
            DeliveryEffect::SetAlarm { at } => EffectAction::SetAlarm { at },
        }
    }
}
impl WireKind for SessionKind {
    fn call(value: Self::Call) -> WireCall {
        WireCall::Session(value)
    }
    fn result(value: Self::Result) -> WireResult {
        WireResult::Session(value)
    }
    fn decode_result(value: WireResult) -> Result<Self::Result, CentralError> {
        match value {
            WireResult::Session(value) => Ok(value),
            _ => Err(CentralError::ReplyContract),
        }
    }
}
impl WireKind for RegistryKind {
    fn call(value: Self::Call) -> WireCall {
        WireCall::Registry(value)
    }
    fn result(value: Self::Result) -> WireResult {
        WireResult::Registry(value)
    }
    fn decode_result(value: WireResult) -> Result<Self::Result, CentralError> {
        match value {
            WireResult::Registry(value) => Ok(value),
            _ => Err(CentralError::ReplyContract),
        }
    }
}
impl WireKind for CopyKind {
    fn call(value: Self::Call) -> WireCall {
        WireCall::Copy(value)
    }
    fn result(value: Self::Result) -> WireResult {
        WireResult::Copy(value)
    }
    fn decode_result(value: WireResult) -> Result<Self::Result, CentralError> {
        match value {
            WireResult::Copy(value) => Ok(value),
            _ => Err(CentralError::ReplyContract),
        }
    }
}
impl WireKind for FleetKind {
    fn call(value: Self::Call) -> WireCall {
        WireCall::Fleet(value)
    }
    fn result(value: Self::Result) -> WireResult {
        WireResult::Fleet(value)
    }
    fn decode_result(value: WireResult) -> Result<Self::Result, CentralError> {
        match value {
            WireResult::Fleet(value) => Ok(value),
            _ => Err(CentralError::ReplyContract),
        }
    }
}
impl WireKind for DeliveryKind {
    fn call(value: Self::Call) -> WireCall {
        WireCall::Delivery(value)
    }
    fn result(value: Self::Result) -> WireResult {
        WireResult::Delivery(value)
    }
    fn decode_result(value: WireResult) -> Result<Self::Result, CentralError> {
        match value {
            WireResult::Delivery(value) => Ok(value),
            _ => Err(CentralError::ReplyContract),
        }
    }
}
impl ObjectEffects {
    #[must_use]
    pub const fn kind(&self) -> CallerKind {
        match self {
            Self::Session(_) => CallerKind::Session,
            Self::Registry(_) => CallerKind::Registry,
            Self::Copy(_) => CallerKind::Copy,
            Self::Fleet(_) => CallerKind::Fleet,
            Self::Delivery(_) => CallerKind::Delivery,
        }
    }
}
impl ObjectAddress {
    #[must_use]
    pub const fn primary_face(&self) -> CallerKind {
        match self {
            Self::Connection(_) => CallerKind::Session,
            Self::Registry => CallerKind::Registry,
            Self::Copy(_) => CallerKind::Copy,
            Self::Fleet => CallerKind::Fleet,
        }
    }
}
