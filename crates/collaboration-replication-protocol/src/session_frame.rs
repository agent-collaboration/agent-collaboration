use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    BoardId, Epoch, FrameId, InstallationId, Piece, ProjectCopyRead, ProjectId,
    ReplicationPosition, RequestId, SchemaSupport, SessionId, SnapshotCursor, TailDigest,
    TailReportRow, TransactionKey, TransferId,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FleetPayload(Value);

impl FleetPayload {
    #[must_use]
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    #[must_use]
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    #[must_use]
    pub fn into_value(self) -> Value {
        self.0
    }
}

impl From<Value> for FleetPayload {
    fn from(value: Value) -> Self {
        Self::new(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FleetPosition(u64);

impl FleetPosition {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u64> for FleetPosition {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct NoticePosition(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("notice position must be at least 1")]
pub struct NoticePositionError;

impl NoticePosition {
    pub fn new(value: u64) -> Result<Self, NoticePositionError> {
        Self::try_from(value)
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for NoticePosition {
    type Error = NoticePositionError;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        (value > 0)
            .then_some(Self(value))
            .ok_or(NoticePositionError)
    }
}
impl From<NoticePosition> for u64 {
    fn from(value: NoticePosition) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ReadCursor {
    Fleet(FleetPosition),
    Notices(NoticePosition),
    Snapshot(SnapshotCursor),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetRequestEnvelope {
    pub request_id: RequestId,
    pub operation: FleetPayload,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ReadScope {
    TailReports { project: ProjectId },
    FleetRecords,
    Directory { query: FleetPayload },
    ProjectCopy { project: ProjectId },
    Notices,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSessionState {
    pub project: ProjectId,
    pub acked: ReplicationPosition,
    pub lease: LeaseStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Notice {
    pub position: NoticePosition,
    pub kind: NoticeKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ReadPage {
    TailReports {
        reports: Vec<TailReportRow>,
        as_of: DateTime<Utc>,
    },
    FleetRecords {
        records: Vec<FleetPayload>,
        position: FleetPosition,
        as_of: DateTime<Utc>,
    },
    Directory {
        entries: Vec<FleetPayload>,
        as_of: DateTime<Utc>,
    },
    ProjectCopy(ProjectCopyRead),
    Notices {
        notices: Vec<Notice>,
        as_of: DateTime<Utc>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LeaseState {
    Held,
    Expired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseStatus {
    pub project: ProjectId,
    pub epoch: Epoch,
    pub holder: InstallationId,
    pub state: LeaseState,
    pub expires_at_registry: DateTime<Utc>,
    pub remaining_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MoveRefusalReason {
    HolderUnreachable,
    MigrationPending,
    TransferInProgress,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MoveResult {
    Unavailable {
        transfer: TransferId,
        reason: crate::CopyUnavailableReason,
    },
    Moved {
        new_epoch: Epoch,
        start: ReplicationPosition,
    },
    Pending {
        transfer_id: TransferId,
    },
    Refused {
        reason: MoveRefusalReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TakeoverPreview {
    pub holder: InstallationId,
    pub epoch: Epoch,
    pub last_seen: DateTime<Utc>,
    pub acked: ReplicationPosition,
    pub tail_estimate_ms: u64,
    pub confirm_token: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum TakeoverRefusalReason {
    PreviewStale,
    TransferInProgress,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum TakeoverResult {
    Unavailable {
        transfer: TransferId,
        reason: crate::CopyUnavailableReason,
    },
    Granted {
        new_epoch: Epoch,
        start: ReplicationPosition,
    },
    Pending {
        transfer_id: TransferId,
    },
    Refused {
        reason: TakeoverRefusalReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SessionRefusal {
    Retired,
    CopyUnavailable {
        project: ProjectId,
        reason: crate::CopyUnavailableReason,
    },
    TailConflict {
        existing: TailDigest,
    },
    Archived,
    InvalidShape {
        field: String,
    },
    AlreadyHeld {
        holder: InstallationId,
    },
    SchemaTooNew {
        store: crate::SchemaIdentity,
        supported: Option<u64>,
    },
    NotFound,
    NotHeld,
    SchemaUnsupported {
        form: crate::SchemaStatementForm,
    },
    StaleEpoch {
        current_epoch: Epoch,
        epoch_ended_at: ReplicationPosition,
    },
    NotWriter {
        current: Option<Epoch>,
    },
    PositionGap {
        acked: ReplicationPosition,
    },
    PieceConflict,
    NotInitialized,
    CentralCopyFull,
    ProjectRejected {
        reason: String,
    },
    CursorExpired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CredentialRejection {
    Unknown,
    Expired,
    Revoked,
    NotMachine,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MigrationState {
    Pending,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ReleaseRefusal {
    SchemaTooNew {
        store: crate::SchemaIdentity,
        supported: Option<u64>,
    },
    MigrationPending {
        migration: u64,
        state: MigrationState,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum HolderReleaseRefusal {
    MigrationPending {
        migration: u64,
        state: MigrationState,
    },
}
impl From<HolderReleaseRefusal> for ReleaseRefusal {
    fn from(reason: HolderReleaseRefusal) -> Self {
        match reason {
            HolderReleaseRefusal::MigrationPending { migration, state } => {
                Self::MigrationPending { migration, state }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ReleaseOutcome {
    Final(ReplicationPosition),
    Refused(HolderReleaseRefusal),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum SessionRequest {
    Claim {
        attempt: RequestId,
        project: ProjectId,
    },
    Forward {
        project: ProjectId,
        post: crate::ForwardedPost,
    },
    Piece(Piece),
    Renew {
        attempt: RequestId,
        project: ProjectId,
        epoch: Epoch,
    },
    Release {
        project: ProjectId,
        epoch: Epoch,
        transfer: TransferId,
        outcome: ReleaseOutcome,
    },
    ReserveFirstEpoch {
        project: ProjectId,
    },
    TailReport {
        project: ProjectId,
        epoch: Epoch,
        request_ids: Vec<RequestId>,
        digest: TailDigest,
    },
    Fleet(FleetRequestEnvelope),
    Read {
        scope: ReadScope,
        after: Option<ReadCursor>,
    },
    ArchiveSeen {
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
pub enum SessionAnswer {
    Unavailable {
        subject: crate::BlockedSubject,
        reason: crate::CopyUnavailableReason,
    },
    Claimed {
        epoch: Epoch,
        expires_at: DateTime<Utc>,
        start: ReplicationPosition,
    },
    Forwarded(crate::ForwardOutcome),
    Acked {
        through: ReplicationPosition,
    },
    PartStaged {
        transaction: TransactionKey,
        index: u32,
        acked: ReplicationPosition,
    },
    Renewed {
        expires_at: DateTime<Utc>,
        transfer_in_progress: Option<TransferId>,
        replayed: bool,
        current: LeaseStatus,
    },
    Released {
        #[serde(rename = "final")]
        final_position: ReplicationPosition,
    },
    ReleaseRefused {
        transfer: TransferId,
        reason: ReleaseRefusal,
    },
    FirstLease {
        epoch: Epoch,
        expires_at: DateTime<Utc>,
    },
    TailStored,
    ArchiveReport(FleetPayload),
    FleetDecision {
        request_id: RequestId,
        decision: FleetPayload,
    },
    ReadPage(ReadPage),
    Refused(SessionRefusal),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum NoticeKind {
    FleetFeedAdvanced {
        through: FleetPosition,
    },
    LeaseSuperseded {
        project: ProjectId,
        epoch: Epoch,
    },
    LeaseGranted {
        project: ProjectId,
        epoch: Epoch,
    },
    ReleaseRequested {
        project: ProjectId,
        transfer: TransferId,
    },
    BoardArchiving {
        board: BoardId,
    },
    RejectedTailReported {
        project: ProjectId,
        epoch: Epoch,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum SessionCloseReason {
    CredentialRejected(CredentialRejection),
    ConnectionReplaced,
    MalformedFrame { detail: MalformedFrameDetail },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MalformedFrameDetail {
    Undecodable,
    RequestWithoutFrameId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MachineFrame {
    ForwardAnswer {
        forward_id: crate::ForwardId,
        reply: crate::BoardPayload,
    },
    Hello {
        installation: InstallationId,
        session_id: SessionId,
        schema_support: SchemaSupport,
    },
    Request {
        frame_id: FrameId,
        body: SessionRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum CentralFrame {
    Forwarded {
        forward_id: crate::ForwardId,
        origin: InstallationId,
        origin_label: String,
        project: ProjectId,
        post: crate::ForwardedPost,
    },
    Welcome {
        session_id: SessionId,
        projects: Vec<ProjectSessionState>,
    },
    Answer {
        frame_id: FrameId,
        body: SessionAnswer,
    },
    Notice {
        position: NoticePosition,
        kind: NoticeKind,
    },
    Closed {
        reason: SessionCloseReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArchiveSeen {
    pub board: BoardId,
    pub project: ProjectId,
    pub position: ReplicationPosition,
}
