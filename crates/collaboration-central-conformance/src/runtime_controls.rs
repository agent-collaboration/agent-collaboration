//! Conformance-only commands address the same physical object as normal traffic.

use collaboration_central_core::{ObjectAddress, RegistryTime};
use fleet_sync_protocol::FrameId;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeControlRequest {
    pub address: ObjectAddress,
    pub command: RuntimeControl,
}
impl RuntimeControlRequest {
    pub fn validate(&self) -> Result<(), RuntimeControlFailure> {
        match (&self.command, &self.address) {
            (RuntimeControl::CopyFull { .. }, ObjectAddress::Copy(_))
            | (RuntimeControl::SmallMailboxCapacity, ObjectAddress::Mailbox(_))
            | (
                RuntimeControl::DropNextAnswer { .. } | RuntimeControl::AwaitDroppedAnswer { .. },
                ObjectAddress::Connection(_),
            )
            | (
                RuntimeControl::SetTime { .. }
                | RuntimeControl::HoldNextDispatch { .. }
                | RuntimeControl::HoldNextSelectedDispatch { .. }
                | RuntimeControl::AwaitHeldDispatch
                | RuntimeControl::ResumeDispatch
                | RuntimeControl::ResumeDispatchAndHoldNext { .. }
                | RuntimeControl::ResumeDispatchAndHoldNextSelected { .. }
                | RuntimeControl::DropNextObjectReply { .. }
                | RuntimeControl::DropNextSelectedObjectReply { .. }
                | RuntimeControl::AwaitDroppedObjectReply
                | RuntimeControl::Observe,
                _,
            ) => Ok(()),
            _ => Err(RuntimeControlFailure::WrongObjectKind),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeControl {
    SmallMailboxCapacity,
    SetTime {
        now: RegistryTime,
    },
    CopyFull {
        after_writes: NonZeroU64,
    },
    DropNextAnswer {
        frame: FrameId,
    },
    AwaitDroppedAnswer {
        frame: FrameId,
    },
    HoldNextDispatch {
        destination: ObjectAddress,
    },
    HoldNextSelectedDispatch {
        destination: ObjectAddress,
        call: DispatchCallSelector,
    },
    AwaitHeldDispatch,
    ResumeDispatch,
    ResumeDispatchAndHoldNext {
        destination: ObjectAddress,
    },
    ResumeDispatchAndHoldNextSelected {
        destination: ObjectAddress,
        call: DispatchCallSelector,
    },
    DropNextObjectReply {
        destination: ObjectAddress,
    },
    DropNextSelectedObjectReply {
        destination: ObjectAddress,
        call: DispatchCallSelector,
    },
    AwaitDroppedObjectReply,
    Observe,
}

/// Closed outer invocation kinds used to select a fault amid concurrent traffic.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DispatchCallSelector {
    MailboxSubmit,
    InstallationPeer,
    ForwardRoute,
    CopyWrite,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ClockObservation {
    Wall,
    Pinned { now: RegistryTime },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum StorageFaultObservation {
    Idle,
    Armed {
        after_writes: NonZeroU64,
    },
    Triggered {
        after_writes: NonZeroU64,
        written: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AnswerFaultObservation {
    Idle,
    Armed { frame: FrameId },
    Dropped { frame: FrameId },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DispatchFaultObservation {
    Idle,
    Armed {
        destination: ObjectAddress,
    },
    Held {
        destination: ObjectAddress,
        request: Vec<u8>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ObjectReplyFaultObservation {
    Idle,
    Armed {
        destination: ObjectAddress,
    },
    Dropped {
        destination: ObjectAddress,
        request: Vec<u8>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeFaultObservation {
    pub clock: ClockObservation,
    pub storage: StorageFaultObservation,
    pub answer: AnswerFaultObservation,
    pub dispatch: DispatchFaultObservation,
    pub object_reply: ObjectReplyFaultObservation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeControlFailure {
    WrongObjectKind,
    Poisoned,
    NotArmed,
    AlreadyArmed,
    NotHeld,
    DifferentFrame,
}
impl std::fmt::Display for RuntimeControlFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "runtime conformance control: {self:?}")
    }
}
impl std::error::Error for RuntimeControlFailure {}

/// Only the outer answer identity is needed to select an opaque outgoing frame.
/// The runtime keeps transporting the original bytes, without decoding its body.
#[derive(Deserialize)]
pub struct OutboundFrameIdentity {
    kind: OutboundFrameClass,
    value: Option<OutboundFrameValue>,
}
#[derive(Deserialize)]
struct OutboundFrameValue {
    frame: Option<FrameId>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum OutboundFrameClass {
    Answer,
    #[serde(other)]
    Other,
}
impl OutboundFrameIdentity {
    pub fn answer_frame(self) -> Option<FrameId> {
        match self.kind {
            OutboundFrameClass::Answer => self.value.and_then(|value| value.frame),
            OutboundFrameClass::Other => None,
        }
    }
}
