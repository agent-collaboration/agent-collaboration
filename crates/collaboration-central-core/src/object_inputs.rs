use crate::*;
use collaboration_replication_protocol::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum SessionReply {
    Registry(RegistryResult),
    Copy(CopyResult),
    Fleet(FleetResult),
    Delivery(DeliveryResult),
}
impl From<RegistryResult> for SessionReply {
    fn from(value: RegistryResult) -> Self {
        Self::Registry(value)
    }
}
impl From<CopyResult> for SessionReply {
    fn from(value: CopyResult) -> Self {
        Self::Copy(value)
    }
}
impl From<FleetResult> for SessionReply {
    fn from(value: FleetResult) -> Self {
        Self::Fleet(value)
    }
}
impl From<DeliveryResult> for SessionReply {
    fn from(value: DeliveryResult) -> Self {
        Self::Delivery(value)
    }
}
impl TryFrom<WireResult> for SessionReply {
    type Error = CentralError;
    fn try_from(value: WireResult) -> Result<Self, Self::Error> {
        match value {
            WireResult::Registry(value) => Ok(Self::Registry(value)),
            WireResult::Copy(value) => Ok(Self::Copy(value)),
            WireResult::Fleet(value) => Ok(Self::Fleet(value)),
            WireResult::Delivery(value) => Ok(Self::Delivery(value)),
            _ => Err(CentralError::ReplyContract),
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
pub enum RegistryReply {
    Copy(CopyResult),
    Fleet(FleetResult),
    Delivery(DeliveryResult),
}
impl From<CopyResult> for RegistryReply {
    fn from(value: CopyResult) -> Self {
        Self::Copy(value)
    }
}
impl From<FleetResult> for RegistryReply {
    fn from(value: FleetResult) -> Self {
        Self::Fleet(value)
    }
}
impl From<DeliveryResult> for RegistryReply {
    fn from(value: DeliveryResult) -> Self {
        Self::Delivery(value)
    }
}
impl TryFrom<WireResult> for RegistryReply {
    type Error = CentralError;
    fn try_from(value: WireResult) -> Result<Self, Self::Error> {
        match value {
            WireResult::Copy(value) => Ok(Self::Copy(value)),
            WireResult::Fleet(value) => Ok(Self::Fleet(value)),
            WireResult::Delivery(value) => Ok(Self::Delivery(value)),
            _ => Err(CentralError::ReplyContract),
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
pub enum CopyReply {
    Fleet(FleetResult),
}
impl From<FleetResult> for CopyReply {
    fn from(value: FleetResult) -> Self {
        Self::Fleet(value)
    }
}
impl TryFrom<WireResult> for CopyReply {
    type Error = CentralError;
    fn try_from(value: WireResult) -> Result<Self, Self::Error> {
        match value {
            WireResult::Fleet(value) => Ok(Self::Fleet(value)),
            _ => Err(CentralError::ReplyContract),
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
pub enum FleetReply {
    Delivery(DeliveryResult),
}
impl From<DeliveryResult> for FleetReply {
    fn from(value: DeliveryResult) -> Self {
        Self::Delivery(value)
    }
}
impl TryFrom<WireResult> for FleetReply {
    type Error = CentralError;
    fn try_from(value: WireResult) -> Result<Self, Self::Error> {
        match value {
            WireResult::Delivery(value) => Ok(Self::Delivery(value)),
            _ => Err(CentralError::ReplyContract),
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
pub enum ObjectInput<Call, Reply> {
    Call {
        from: ObjectAddress,
        call_id: ObjectCallId,
        call: Box<Call>,
    },
    CallResult {
        call_id: ObjectCallId,
        result: Box<Reply>,
        carry: Option<DispatchCarry>,
    },
    CallFailed {
        call_id: ObjectCallId,
        carry: Option<DispatchCarry>,
    },
    Alarm,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum NoReply {}
pub type RegistryInput = ObjectInput<RegistryCall, RegistryReply>;
/// Health's measured fact exists only after the target shell has received the
/// unit transport request. This core input has no wire deserializer.
#[derive(Clone, Debug, PartialEq)]
pub enum CopyInput {
    Call {
        from: ObjectAddress,
        call_id: ObjectCallId,
        call: Box<CopyCall>,
    },
    CallResult {
        call_id: ObjectCallId,
        result: Box<CopyReply>,
        carry: Option<DispatchCarry>,
    },
    CallFailed {
        call_id: ObjectCallId,
        carry: Option<DispatchCarry>,
    },
    Alarm,
    Health {
        call_id: ObjectCallId,
        storage_bytes: u64,
    },
}
pub type FleetInput = ObjectInput<FleetCall, FleetReply>;
pub type DeliveryInput = ObjectInput<DeliveryCall, NoReply>;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum SessionInput {
    DecodeFailure {
        connection: ConnectionId,
        failure: MachineFrameDecodeFailure,
    },
    #[cfg(feature = "conformance")]
    ConformanceClose {
        call_id: ObjectCallId,
        reason: CredentialRejection,
    },

    Hello {
        connection: ConnectionId,
        credential: CredentialStatus,
        installation: InstallationId,
        session_id: SessionId,
        schema_support: SchemaSupport,
    },
    Frame {
        connection: ConnectionId,
        frame_id: FrameId,
        request: SessionRequest,
    },
    ForwardAnswer {
        connection: ConnectionId,
        forward_id: collaboration_replication_protocol::ForwardId,
        reply: collaboration_replication_protocol::BoardPayload,
    },
    Disconnected {
        connection: ConnectionId,
    },
    CallResult {
        call_id: ObjectCallId,
        result: Box<SessionReply>,
        carry: Option<DispatchCarry>,
    },
    CallFailed {
        call_id: ObjectCallId,
        carry: Option<DispatchCarry>,
    },
    Alarm,
}
impl TryFrom<SessionCall> for SessionInput {
    type Error = CentralError;
    fn try_from(value: SessionCall) -> Result<Self, Self::Error> {
        Ok(match value {
            SessionCall::DecodeFailure {
                connection,
                failure,
            } => Self::DecodeFailure {
                connection,
                failure,
            },
            #[cfg(feature = "conformance")]
            SessionCall::ConformanceClose { .. } => return Err(CentralError::ReplyContract),
            SessionCall::Hello {
                connection,
                credential,
                installation,
                session_id,
                schema_support,
            } => Self::Hello {
                connection,
                credential,
                installation,
                session_id,
                schema_support,
            },
            SessionCall::Frame {
                connection,
                frame_id,
                request,
            } => Self::Frame {
                connection,
                frame_id,
                request,
            },
            SessionCall::ForwardAnswer {
                connection,
                forward_id,
                reply,
            } => Self::ForwardAnswer {
                connection,
                forward_id,
                reply,
            },
            SessionCall::Disconnected { connection } => Self::Disconnected { connection },
        })
    }
}
