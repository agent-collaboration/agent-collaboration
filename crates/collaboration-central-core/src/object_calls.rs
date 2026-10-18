//! Per-face inbound calls and results, shared by native and Cloudflare shells.
use crate::{
    AdminAuthority, AdminRequest, AdminResult, ConnectionId, CopyStatus, CredentialStatus,
    FenceExpectation, FenceResult, RegistrationResult, ReservationSource, SessionRegistration,
};
use chrono::{DateTime, Utc};
use collaboration_replication_protocol::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RegistryCall {
    VerifyTail {
        project: ProjectId,
        epoch: Epoch,
        installation: InstallationId,
    },
    Claim {
        attempt: RequestId,
        project: ProjectId,
        claimant: InstallationId,
    },
    RecordUnheld(crate::UnheldProject),
    ForwardRoute {
        project: ProjectId,
        sender: InstallationId,
    },
    Renew {
        installation: InstallationId,
        attempt: RequestId,
        project: ProjectId,
        epoch: Epoch,
    },
    Release {
        installation: InstallationId,
        project: ProjectId,
        epoch: Epoch,
        transfer: TransferId,
        outcome: ReleaseOutcome,
    },
    ReserveFirstEpoch {
        project: ProjectId,
        source: ReservationSource,
    },
    RegisterSession(SessionRegistration),
    SessionDisconnected {
        installation: InstallationId,
        connection: ConnectionId,
    },
    AuthenticateMachine {
        credential_hash: String,
        access_client_id: crate::AccessClientId,
    },
    Admin {
        authority: AdminAuthority,
        request: AdminRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RegistryResult {
    TailVerification(crate::TailVerificationResult),
    Claim(SessionAnswer),
    UnheldRecorded,
    ForwardRoute(crate::ForwardRouteResult),
    Renew(SessionAnswer),
    Release(SessionAnswer),
    FirstLease(SessionAnswer),
    Registered(RegistrationResult),
    SessionDisconnected,
    Authenticated(CredentialStatus),
    Admin(AdminResult),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum CopyCall {
    Ops(crate::CopyOpsCall),
    ImportSeed {
        project: ProjectId,
        upload: Box<collaboration_replication_protocol::ProjectImportPage>,
    },
    ApplyPiece {
        piece: Piece,
        sender: InstallationId,
    },
    Fence {
        transfer_id: TransferId,
        new_epoch: Epoch,
        expect: FenceExpectation,
        target_schema_max: u64,
        holder: InstallationId,
    },
    Initialize {
        project: ProjectId,
        epoch: Epoch,
        holder: InstallationId,
    },
    CopyStatus {
        project: ProjectId,
    },
    SnapshotPage {
        cursor: Option<SnapshotCursor>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum CopyResult {
    Retired,
    Health(Box<crate::CopyHealth>),
    Repaired,
    Migrated(crate::CopyMigrationResult),
    Unavailable(CopyUnavailableReason),
    ImportSeed(crate::ImportSeedResult),
    ApplyPiece(SessionAnswer),
    Fence(FenceResult),
    Initialized(CopyStatus),
    CopyStatus(Option<CopyStatus>),
    SnapshotPage(SessionAnswer),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum FleetCall {
    AdvanceLeaseIncarnation {
        at_least: u64,
    },
    ReserveClaim {
        project: ProjectId,
        attempt: RequestId,
    },
    ReleaseClaim {
        project: ProjectId,
        attempt: RequestId,
    },
    Import {
        bundle: Box<fleet_admission::FleetImportBundle>,
    },
    DirectoryUpdate(crate::DirectoryUpdate),
    StoreTail {
        project: ProjectId,
        epoch: Epoch,
        request_ids: Vec<RequestId>,
        digest: TailDigest,
        current_holder: InstallationId,
    },
    Fleet {
        installation: InstallationId,
        request: FleetRequestEnvelope,
    },
    Read {
        installation: InstallationId,
        scope: ReadScope,
        after: Option<ReadCursor>,
    },
    ArchiveSeen {
        installation: InstallationId,
        board: BoardId,
        project: ProjectId,
        position: ReplicationPosition,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum FleetResult {
    LeaseIncarnationAdvanced(crate::RegistryIncarnation),
    LeaseUpdated(crate::LeaseUpdateResult),
    ClaimReserved(crate::ReserveClaimResult),
    ClaimReleased,
    ProjectAccepted {
        project: ProjectId,
        answer: SessionAnswer,
    },
    Imported(fleet_admission::FleetImportResult),
    DirectoryUpdated,
    TailReport(SessionAnswer),
    Fleet(SessionAnswer),
    Read(SessionAnswer),
    ArchiveSeen(SessionAnswer),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum DeliveryCall {
    DeliverForward {
        forward_id: collaboration_replication_protocol::ForwardId,
        origin: InstallationId,
        origin_label: String,
        project: ProjectId,
        post: collaboration_replication_protocol::ForwardedPost,
        deadline: DateTime<Utc>,
    },
    ForwardAnswered {
        forward_id: collaboration_replication_protocol::ForwardId,
        project: ProjectId,
        outcome: collaboration_replication_protocol::ForwardOutcome,
    },
    CloseSession {
        reason: collaboration_replication_protocol::CredentialRejection,
    },
    DeliverNotice(NoticeKind),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum DeliveryResult {
    DeliverForward(crate::ForwardDeliveryResult),
    ForwardAck,
    SessionClosed,
    NoticeDelivered,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum SessionCall {
    DecodeFailure {
        connection: ConnectionId,
        failure: collaboration_replication_protocol::MachineFrameDecodeFailure,
    },
    #[cfg(feature = "conformance")]
    ConformanceClose {
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum SessionResult {
    #[cfg(feature = "conformance")]
    ConformanceClosed,
}

impl RegistryCall {
    pub(crate) fn matches_result(&self, result: &RegistryResult) -> bool {
        match self {
            Self::VerifyTail { .. } => matches!(result, RegistryResult::TailVerification(_)),
            Self::Claim { .. } => matches!(result, RegistryResult::Claim(_)),
            Self::RecordUnheld(_) => matches!(result, RegistryResult::UnheldRecorded),
            Self::ForwardRoute { .. } => matches!(result, RegistryResult::ForwardRoute(_)),
            Self::Renew { .. } => matches!(result, RegistryResult::Renew(_)),
            Self::Release { .. } => matches!(result, RegistryResult::Release(_)),
            Self::ReserveFirstEpoch { .. } => matches!(result, RegistryResult::FirstLease(_)),
            Self::RegisterSession(_) => matches!(result, RegistryResult::Registered(_)),
            Self::SessionDisconnected { .. } => {
                matches!(result, RegistryResult::SessionDisconnected)
            }
            Self::AuthenticateMachine { .. } => matches!(result, RegistryResult::Authenticated(_)),
            Self::Admin { .. } => matches!(result, RegistryResult::Admin(_)),
        }
    }
}

impl CopyCall {
    pub(crate) fn matches_result(&self, result: &CopyResult) -> bool {
        if matches!(result, CopyResult::Unavailable(_)) {
            return true;
        }
        if matches!(result, CopyResult::Retired) {
            return matches!(
                self,
                Self::Ops(crate::CopyOpsCall::Repair {})
                    | Self::Initialize { .. }
                    | Self::Fence { .. }
                    | Self::CopyStatus { .. }
            );
        }
        match self {
            Self::Ops(crate::CopyOpsCall::Health {}) => matches!(result, CopyResult::Health(_)),
            Self::Ops(crate::CopyOpsCall::Repair {}) => matches!(result, CopyResult::Repaired),
            Self::Ops(crate::CopyOpsCall::Migrate {}) => matches!(result, CopyResult::Migrated(_)),
            Self::ImportSeed { .. } => matches!(result, CopyResult::ImportSeed(_)),
            Self::ApplyPiece { .. } => matches!(result, CopyResult::ApplyPiece(_)),
            Self::Fence { .. } => matches!(result, CopyResult::Fence(_)),
            Self::Initialize { .. } => matches!(result, CopyResult::Initialized(_)),
            Self::CopyStatus { .. } => matches!(result, CopyResult::CopyStatus(_)),
            Self::SnapshotPage { .. } => matches!(result, CopyResult::SnapshotPage(_)),
        }
    }
}

impl FleetCall {
    pub(crate) fn matches_result(&self, result: &FleetResult) -> bool {
        match self {
            Self::AdvanceLeaseIncarnation { .. } => {
                matches!(result, FleetResult::LeaseIncarnationAdvanced(_))
            }
            Self::ReserveClaim { .. } => matches!(result, FleetResult::ClaimReserved(_)),
            Self::ReleaseClaim { .. } => matches!(result, FleetResult::ClaimReleased),
            Self::Import { .. } => matches!(result, FleetResult::Imported(_)),
            Self::DirectoryUpdate(crate::DirectoryUpdate::Summary { .. }) => {
                matches!(result, FleetResult::DirectoryUpdated)
            }
            Self::DirectoryUpdate(
                crate::DirectoryUpdate::Lease { .. } | crate::DirectoryUpdate::ClaimGranted { .. },
            ) => matches!(result, FleetResult::LeaseUpdated(_)),
            Self::StoreTail { .. } => matches!(result, FleetResult::TailReport(_)),
            Self::Fleet { .. } => matches!(
                result,
                FleetResult::Fleet(_) | FleetResult::ProjectAccepted { .. }
            ),
            Self::Read { .. } => matches!(result, FleetResult::Read(_)),
            Self::ArchiveSeen { .. } => matches!(result, FleetResult::ArchiveSeen(_)),
        }
    }
}

impl DeliveryCall {
    pub(crate) fn matches_result(&self, result: &DeliveryResult) -> bool {
        match self {
            Self::DeliverForward { .. } => matches!(result, DeliveryResult::DeliverForward(_)),
            Self::ForwardAnswered { .. } => matches!(result, DeliveryResult::ForwardAck),
            Self::CloseSession { .. } => matches!(result, DeliveryResult::SessionClosed),
            Self::DeliverNotice(_) => matches!(result, DeliveryResult::NoticeDelivered),
        }
    }
}
