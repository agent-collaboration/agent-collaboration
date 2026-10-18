//! Serialized/trace envelopes. Core rule handlers use per-kind calls/effects.
use crate::*;
use collaboration_replication_protocol::{CentralFrame, InstallationId, NoticeKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WireCall {
    Session(SessionCall),
    Registry(RegistryCall),
    Copy(CopyCall),
    Fleet(FleetCall),
    Delivery(DeliveryCall),
}
impl From<SessionCall> for WireCall {
    fn from(value: SessionCall) -> Self {
        Self::Session(value)
    }
}
impl From<RegistryCall> for WireCall {
    fn from(value: RegistryCall) -> Self {
        Self::Registry(value)
    }
}
impl From<CopyCall> for WireCall {
    fn from(value: CopyCall) -> Self {
        Self::Copy(value)
    }
}
impl From<FleetCall> for WireCall {
    fn from(value: FleetCall) -> Self {
        Self::Fleet(value)
    }
}
impl From<DeliveryCall> for WireCall {
    fn from(value: DeliveryCall) -> Self {
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
pub enum WireResult {
    Session(SessionResult),
    Registry(RegistryResult),
    Copy(CopyResult),
    Fleet(FleetResult),
    Delivery(DeliveryResult),
}
impl From<SessionResult> for WireResult {
    fn from(value: SessionResult) -> Self {
        Self::Session(value)
    }
}
impl From<RegistryResult> for WireResult {
    fn from(value: RegistryResult) -> Self {
        Self::Registry(value)
    }
}
impl From<CopyResult> for WireResult {
    fn from(value: CopyResult) -> Self {
        Self::Copy(value)
    }
}
impl From<FleetResult> for WireResult {
    fn from(value: FleetResult) -> Self {
        Self::Fleet(value)
    }
}
impl From<DeliveryResult> for WireResult {
    fn from(value: DeliveryResult) -> Self {
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
pub enum WireInput {
    Call {
        from: ObjectAddress,
        call_id: ObjectCallId,
        call: Box<WireCall>,
    },
    CallResult {
        source: CallerKind,
        call_id: ObjectCallId,
        result: Box<WireResult>,
        carry: Option<DispatchCarry>,
    },
    CallFailed {
        source: CallerKind,
        call_id: ObjectCallId,
        carry: Option<DispatchCarry>,
    },
    Session(SessionCall),
    Alarm {
        face: CallerKind,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WireEffect {
    Send {
        to: ConnectionId,
        frame: CentralFrame,
    },
    Notify {
        installation: InstallationId,
        kind: NoticeKind,
    },
    Dispatch {
        call_id: ObjectCallId,
        to: ObjectAddress,
        call: WireCall,
        carry: Option<crate::DispatchCarry>,
    },
    Return {
        call_id: ObjectCallId,
        result: WireResult,
    },
    SetAlarm {
        at: RegistryTime,
    },
}

impl WireCall {
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
    #[must_use]
    pub fn matches_result(&self, result: &WireResult) -> bool {
        match self {
            Self::Session(call) => {
                #[cfg(feature = "conformance")]
                {
                    matches!(
                        (call, result),
                        (
                            SessionCall::ConformanceClose { .. },
                            WireResult::Session(SessionResult::ConformanceClosed)
                        )
                    )
                }
                #[cfg(not(feature = "conformance"))]
                {
                    let _ = call;
                    false
                }
            }
            Self::Registry(call) => match result {
                WireResult::Registry(result) => call.matches_result(result),
                _ => false,
            },
            Self::Copy(call) => match result {
                WireResult::Copy(result) => call.matches_result(result),
                _ => false,
            },
            Self::Fleet(call) => match result {
                WireResult::Fleet(result) => call.matches_result(result),
                _ => false,
            },
            Self::Delivery(call) => match result {
                WireResult::Delivery(result) => call.matches_result(result),
                _ => false,
            },
        }
    }
}

impl WireInput {
    #[must_use]
    pub fn face(&self) -> CallerKind {
        match self {
            Self::Call { call, .. } => call.kind(),
            Self::Session(_) => CallerKind::Session,
            Self::CallResult { source, .. } | Self::CallFailed { source, .. } => *source,
            Self::Alarm { face } => *face,
        }
    }
}
