use collaboration_central_core::{
    AccessRefusal, ObjectAddress, RegistryTime, WireCall, WireEffect, WireInput, WireResult,
};
use collaboration_replication_protocol::InstallationId;
use serde::{Deserialize, Serialize};
/// Test-build-only platform controls. They never carry a production credential
/// or become a core port; all rule assertions use the real core and engine.
/// `VerifyAccessAssertion` carries one staging assertion as transient input to
/// the edge's own verifier; no shell stores or logs it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ControlRequest {
    ProbeForbiddenEdge,
    HoldNextFence,
    AwaitHeldFence,
    ResumeFence,
    DropNextDirectoryUpdate,
    DirectoryUpdates,
    Call {
        address: ObjectAddress,
        call: Box<WireCall>,
    },
    Step {
        address: ObjectAddress,
        input: Box<WireInput>,
        now: RegistryTime,
    },
    Evict {
        address: ObjectAddress,
    },
    SetTime {
        now: RegistryTime,
    },
    InterruptAfterFence,
    DropNextAnswer {
        installation: InstallationId,
    },
    AwaitDroppedAnswer {
        installation: InstallationId,
    },
    HoldNextDispatch {
        installation: InstallationId,
    },
    AwaitHeldDispatch {
        installation: InstallationId,
    },
    ResumeDispatch {
        installation: InstallationId,
    },
    HoldNextForwardDelivery {
        installation: InstallationId,
    },
    AwaitHeldForwardDelivery {
        installation: InstallationId,
    },
    ResumeForwardDelivery {
        installation: InstallationId,
    },
    CopyFull {
        address: ObjectAddress,
        after_writes: u64,
    },
    PanicAfterWrites {
        address: ObjectAddress,
        after_writes: u64,
    },
    PendingAlarms,
    Corrupt {
        address: ObjectAddress,
        mutation: StorageCorruption,
    },
    VerifyAccessAssertion {
        assertion: String,
        path: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ControlResult {
    CallEdgeProbe(CallEdgeProbe),
    DirectoryUpdates(Vec<collaboration_central_core::DirectoryUpdate>),
    Call(Box<WireResult>),
    Effects(Vec<WireEffect>),
    Complete,
    Alarms(Vec<(ObjectAddress, RegistryTime)>),
    AccessVerification(AccessVerificationOutcome),
    /// The call `HoldNextDispatch` actually held, so a scenario can tell its
    /// own request from any other dispatch that reached the hold first.
    HeldDispatch(Box<WireCall>),
}

/// The verifier's answer for one submitted assertion: the caller class only,
/// never the identity or claims it carried.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum AccessVerificationOutcome {
    Accepted(AccessCallerClass),
    Refused(AccessRefusal),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccessCallerClass {
    Machine,
    Owner,
}

impl AccessVerificationOutcome {
    #[must_use]
    pub fn of(verified: Result<collaboration_central_core::AccessCaller, AccessRefusal>) -> Self {
        match verified {
            Ok(collaboration_central_core::AccessCaller::Machine { .. }) => {
                Self::Accepted(AccessCallerClass::Machine)
            }
            Ok(collaboration_central_core::AccessCaller::Owner { .. }) => {
                Self::Accepted(AccessCallerClass::Owner)
            }
            Err(refusal) => Self::Refused(refusal),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum StorageCorruption {
    RestoreUnheldRegistry {
        unheld: collaboration_central_core::UnheldProject,
        as_of: RegistryTime,
    },
    RemovePieceReceipts,
    ArchiveCopy,
    RemoveCopyState,
    InvalidateCopyFence,
    ForgetRegistryFloor {
        project: collaboration_replication_protocol::ProjectId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum CallEdgeProbe {
    Refused(collaboration_central_core::CallEdgeRefused),
    Accepted,
}
