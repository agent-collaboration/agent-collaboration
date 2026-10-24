//! Owner holder changes preserve native preview and refusal contracts.
use board_sync_protocol::BoardHolderChangeRefusal;
use fleet_sync_protocol::{
    ActorRef, CopyUnavailableReason, Epoch, FailureCode, HolderChangeReason, ReplicationPosition,
    RequestId, RetirementRefusal, SyncTime, TakeoverPreview, TransferId,
};
use message_board::ServiceId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerMove {
    pub transfer_id: TransferId,
    pub to_installation: ServiceId,
    pub reason: HolderChangeReason,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerTakeover {
    pub transfer_id: TransferId,
    pub to_installation: ServiceId,
    pub reason: HolderChangeReason,
    pub preview: TakeoverPreview,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerRetirement {
    pub request_id: RequestId,
    pub expect_fence: Epoch,
    pub expect_acked: ReplicationPosition,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerPreviewSuccess {
    pub preview: TakeoverPreview,
    pub holder_installation: ServiceId,
    pub last_seen: Option<SyncTime>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OwnerTransferAnswer {
    Moved {
        new_epoch: Epoch,
        start: ReplicationPosition,
    },
    Granted {
        new_epoch: Epoch,
        start: ReplicationPosition,
    },
    Pending {
        transfer_id: TransferId,
    },
    Refused(BoardHolderChangeRefusal),
    ConflictingRequest,
    Cancelled {
        by: ActorRef,
    },
    Failed {
        reason: FailureCode,
    },
    Unavailable {
        subject: OwnerBlockedSubject,
        reason: CopyUnavailableReason,
    },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum OwnerBlockedSubject {
    Transfer { transfer: TransferId },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OwnerRetirementPhase {
    Checking,
    Wiping,
    Directory,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OwnerRetirementAnswer {
    Retired {
        at: SyncTime,
    },
    Pending {
        request_id: RequestId,
        phase: OwnerRetirementPhase,
    },
    Refused(RetirementRefusal),
    ConflictingRequest,
    NotFound,
    Unavailable {
        subject: OwnerBlockedSubject,
        reason: CopyUnavailableReason,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum OwnerPreviewRefusal {
    NotFound,
    Retired,
}

/// Preview success retains the native confirmation value without renaming it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OwnerPreviewResponse {
    Ready(OwnerPreviewSuccess),
    Refused(OwnerPreviewRefusal),
}
