//! Effects are constrained by their originating face, before a shell sees them.
use crate::*;
use collaboration_replication_protocol::{CentralFrame, InstallationId, NoticeKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "", rename_all = "camelCase", deny_unknown_fields)]
pub struct Dispatch<To: ObjectKind> {
    pub call_id: ObjectCallId,
    pub to: To::Address,
    pub call: To::Call,
    pub carry: Option<DispatchCarry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum SessionDispatch {
    Registry(Dispatch<RegistryKind>),
    Copy(Dispatch<CopyKind>),
    Fleet(Dispatch<FleetKind>),
    Delivery(Dispatch<DeliveryKind>),
    Notice {
        installation: InstallationId,
        kind: NoticeKind,
    },
}
impl From<Dispatch<RegistryKind>> for SessionDispatch {
    fn from(value: Dispatch<RegistryKind>) -> Self {
        Self::Registry(value)
    }
}
impl From<Dispatch<CopyKind>> for SessionDispatch {
    fn from(value: Dispatch<CopyKind>) -> Self {
        Self::Copy(value)
    }
}
impl From<Dispatch<FleetKind>> for SessionDispatch {
    fn from(value: Dispatch<FleetKind>) -> Self {
        Self::Fleet(value)
    }
}
impl From<Dispatch<DeliveryKind>> for SessionDispatch {
    fn from(value: Dispatch<DeliveryKind>) -> Self {
        Self::Delivery(value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RegistryDispatch {
    Copy(Dispatch<CopyKind>),
    Fleet(Dispatch<FleetKind>),
    Delivery(Dispatch<DeliveryKind>),
    Notice {
        installation: InstallationId,
        kind: NoticeKind,
    },
}
impl From<Dispatch<CopyKind>> for RegistryDispatch {
    fn from(value: Dispatch<CopyKind>) -> Self {
        Self::Copy(value)
    }
}
impl From<Dispatch<FleetKind>> for RegistryDispatch {
    fn from(value: Dispatch<FleetKind>) -> Self {
        Self::Fleet(value)
    }
}
impl From<Dispatch<DeliveryKind>> for RegistryDispatch {
    fn from(value: Dispatch<DeliveryKind>) -> Self {
        Self::Delivery(value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum CopyDispatch {
    Fleet(Dispatch<FleetKind>),
}
impl From<Dispatch<FleetKind>> for CopyDispatch {
    fn from(value: Dispatch<FleetKind>) -> Self {
        Self::Fleet(value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum FleetDispatch {
    Delivery(Dispatch<DeliveryKind>),
}
impl From<Dispatch<DeliveryKind>> for FleetDispatch {
    fn from(value: Dispatch<DeliveryKind>) -> Self {
        Self::Delivery(value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum KindEffect<Outgoing, Result> {
    Send {
        to: ConnectionId,
        frame: CentralFrame,
    },
    Dispatch(Outgoing),
    Return {
        call_id: ObjectCallId,
        result: Result,
    },
    SetAlarm {
        at: RegistryTime,
    },
}
pub type SessionEffect = KindEffect<SessionDispatch, SessionResult>;
pub type RegistryEffect = KindEffect<RegistryDispatch, RegistryResult>;
pub type CopyEffect = KindEffect<CopyDispatch, CopyResult>;
pub type FleetEffect = KindEffect<FleetDispatch, FleetResult>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum DeliveryEffect {
    Send {
        to: ConnectionId,
        frame: CentralFrame,
    },
    Return {
        call_id: ObjectCallId,
        result: DeliveryResult,
    },
    SetAlarm {
        at: RegistryTime,
    },
}
impl From<SessionEffect> for WireEffect {
    fn from(value: SessionEffect) -> Self {
        match value {
            SessionEffect::Send { to, frame } => Self::Send { to, frame },
            SessionEffect::SetAlarm { at } => Self::SetAlarm { at },
            SessionEffect::Return { call_id, result } => Self::Return {
                call_id,
                result: WireResult::Session(result),
            },
            SessionEffect::Dispatch(SessionDispatch::Registry(dispatch)) => Self::Dispatch {
                call_id: dispatch.call_id,
                to: RegistryKind::address(&dispatch.to),
                call: WireCall::Registry(dispatch.call),
                carry: dispatch.carry,
            },
            SessionEffect::Dispatch(SessionDispatch::Copy(dispatch)) => Self::Dispatch {
                call_id: dispatch.call_id,
                to: CopyKind::address(&dispatch.to),
                call: WireCall::Copy(dispatch.call),
                carry: dispatch.carry,
            },
            SessionEffect::Dispatch(SessionDispatch::Fleet(dispatch)) => Self::Dispatch {
                call_id: dispatch.call_id,
                to: FleetKind::address(&dispatch.to),
                call: WireCall::Fleet(dispatch.call),
                carry: dispatch.carry,
            },
            SessionEffect::Dispatch(SessionDispatch::Delivery(dispatch)) => Self::Dispatch {
                call_id: dispatch.call_id,
                to: DeliveryKind::address(&dispatch.to),
                call: WireCall::Delivery(dispatch.call),
                carry: dispatch.carry,
            },
            SessionEffect::Dispatch(SessionDispatch::Notice { installation, kind }) => {
                Self::Notify { installation, kind }
            }
        }
    }
}
impl From<RegistryEffect> for WireEffect {
    fn from(value: RegistryEffect) -> Self {
        match value {
            RegistryEffect::Send { to, frame } => Self::Send { to, frame },
            RegistryEffect::SetAlarm { at } => Self::SetAlarm { at },
            RegistryEffect::Return { call_id, result } => Self::Return {
                call_id,
                result: WireResult::Registry(result),
            },
            RegistryEffect::Dispatch(RegistryDispatch::Copy(dispatch)) => Self::Dispatch {
                call_id: dispatch.call_id,
                to: CopyKind::address(&dispatch.to),
                call: WireCall::Copy(dispatch.call),
                carry: dispatch.carry,
            },
            RegistryEffect::Dispatch(RegistryDispatch::Fleet(dispatch)) => Self::Dispatch {
                call_id: dispatch.call_id,
                to: FleetKind::address(&dispatch.to),
                call: WireCall::Fleet(dispatch.call),
                carry: dispatch.carry,
            },
            RegistryEffect::Dispatch(RegistryDispatch::Delivery(dispatch)) => Self::Dispatch {
                call_id: dispatch.call_id,
                to: DeliveryKind::address(&dispatch.to),
                call: WireCall::Delivery(dispatch.call),
                carry: dispatch.carry,
            },
            RegistryEffect::Dispatch(RegistryDispatch::Notice { installation, kind }) => {
                Self::Notify { installation, kind }
            }
        }
    }
}
impl From<CopyEffect> for WireEffect {
    fn from(value: CopyEffect) -> Self {
        match value {
            CopyEffect::Send { to, frame } => Self::Send { to, frame },
            CopyEffect::SetAlarm { at } => Self::SetAlarm { at },
            CopyEffect::Return { call_id, result } => Self::Return {
                call_id,
                result: WireResult::Copy(result),
            },
            CopyEffect::Dispatch(CopyDispatch::Fleet(dispatch)) => Self::Dispatch {
                call_id: dispatch.call_id,
                to: FleetKind::address(&dispatch.to),
                call: WireCall::Fleet(dispatch.call),
                carry: dispatch.carry,
            },
        }
    }
}
impl From<FleetEffect> for WireEffect {
    fn from(value: FleetEffect) -> Self {
        match value {
            FleetEffect::Send { to, frame } => Self::Send { to, frame },
            FleetEffect::SetAlarm { at } => Self::SetAlarm { at },
            FleetEffect::Return { call_id, result } => Self::Return {
                call_id,
                result: WireResult::Fleet(result),
            },
            FleetEffect::Dispatch(FleetDispatch::Delivery(dispatch)) => Self::Dispatch {
                call_id: dispatch.call_id,
                to: DeliveryKind::address(&dispatch.to),
                call: WireCall::Delivery(dispatch.call),
                carry: dispatch.carry,
            },
        }
    }
}
impl From<DeliveryEffect> for WireEffect {
    fn from(value: DeliveryEffect) -> Self {
        match value {
            DeliveryEffect::Send { to, frame } => Self::Send { to, frame },
            DeliveryEffect::SetAlarm { at } => Self::SetAlarm { at },
            DeliveryEffect::Return { call_id, result } => Self::Return {
                call_id,
                result: WireResult::Delivery(result),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ObjectEffects {
    Session(Vec<SessionEffect>),
    Registry(Vec<RegistryEffect>),
    Copy(Vec<CopyEffect>),
    Fleet(Vec<FleetEffect>),
    Delivery(Vec<DeliveryEffect>),
}
impl ObjectEffects {
    pub fn into_trace(self) -> Vec<WireEffect> {
        match self {
            Self::Session(values) => values.into_iter().map(Into::into).collect(),
            Self::Registry(values) => values.into_iter().map(Into::into).collect(),
            Self::Copy(values) => values.into_iter().map(Into::into).collect(),
            Self::Fleet(values) => values.into_iter().map(Into::into).collect(),
            Self::Delivery(values) => values.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SocketWrite {
    pub to: ConnectionId,
    pub frame: CentralFrame,
}
