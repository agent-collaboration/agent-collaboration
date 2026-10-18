use crate::ObjectCallId;
use chrono::{DateTime, Utc};
use collaboration_replication_protocol::{
    CredentialRejection, Epoch, InstallationId, LeaseStatus, ProjectId, ReleaseRefusal,
    ReplicationPosition, RequestId, SchemaIdentity, SchemaSupport, SessionId, TakeoverPreview,
    TransferId,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConnectionId(RequestId);
impl ConnectionId {
    pub fn new(
        value: impl Into<String>,
    ) -> Result<Self, collaboration_replication_protocol::IdentityError> {
        RequestId::new(value).map(Self)
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum CredentialStatus {
    Machine {
        installation: InstallationId,
        expires_at: DateTime<Utc>,
    },
    Rejected(CredentialRejection),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum AdminAuthority {
    Admin { subject: String },
    Machine { installation: InstallationId },
    Agent,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum AdminRequest {
    ImportProject {
        project: ProjectId,
        upload: Box<collaboration_replication_protocol::ProjectImportPage>,
    },
    EnrollMachine {
        label: String,
        installation: InstallationId,
        access_client_id: crate::AccessClientId,
        credential_hash: String,
        expires_at: DateTime<Utc>,
    },
    RevokeMachine {
        installation: InstallationId,
    },
    TakeoverPreview {
        project: ProjectId,
    },
    Takeover {
        transfer_id: TransferId,
        project: ProjectId,
        to_installation: InstallationId,
        reason: String,
        confirm_token: String,
    },
    Move {
        transfer_id: TransferId,
        project: ProjectId,
        to_installation: InstallationId,
        reason: String,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum AdminRefusal {
    AdminRequired,
    RegistryEpochUnproven,
    ConflictingRequest,
    NotInitialized,
    PreviewStale {
        acked: ReplicationPosition,
    },
    SchemaTooNew {
        store: SchemaIdentity,
        supported: Option<u64>,
    },
    TransferInProgress {
        transfer_id: TransferId,
    },
    HolderUnreachable {
        last_seen: DateTime<Utc>,
    },
    MigrationPending(ReleaseRefusal),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum AdminResult {
    Retired {},
    Unavailable {
        subject: collaboration_replication_protocol::BlockedSubject,
        reason: collaboration_replication_protocol::CopyUnavailableReason,
    },
    ProjectImport(collaboration_replication_protocol::ProjectImportAnswer),
    MachineEnrolled,
    MachineRevoked,
    Preview(TakeoverPreview),
    Granted {
        new_epoch: Epoch,
        start: ReplicationPosition,
    },
    Pending {
        transfer_id: TransferId,
    },
    Refused(AdminRefusal),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum ReservationSource {
    FleetAcceptance { installation: InstallationId },
    Machine { installation: InstallationId },
}
impl ReservationSource {
    pub(crate) fn installation(&self) -> &InstallationId {
        match self {
            Self::FleetAcceptance { installation } | Self::Machine { installation } => installation,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRegistration {
    pub installation: InstallationId,
    pub session_id: SessionId,
    pub connection: ConnectionId,
    pub schema_support: SchemaSupport,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum RegistrationResult {
    Registered { projects: Vec<LeaseStatus> },
    Rejected(CredentialRejection),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegistryLease {
    pub project: ProjectId,
    pub holder: InstallationId,
    pub epoch: Epoch,
    pub expires_at: DateTime<Utc>,
    pub last_renewed_at: DateTime<Utc>,
    pub change: std::num::NonZeroU64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferKind {
    Claim,
    Move,
    Takeover,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FenceAttempt {
    Initial,
    FloorRepair,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum TransferState {
    Reserving {
        new_epoch: Epoch,
        expect: crate::FenceExpectation,
        target_schema_max: u64,
    },
    ClaimGranted {
        epoch: Epoch,
        expires_at: DateTime<Utc>,
        start: ReplicationPosition,
    },
    ClaimRefused(collaboration_replication_protocol::SessionRefusal),
    Draining,
    Fencing {
        new_epoch: Epoch,
        expect: crate::FenceExpectation,
        target_schema_max: Option<u64>,
        attempt: FenceAttempt,
    },
    Granted {
        new_epoch: Epoch,
        start: ReplicationPosition,
    },
    Refused(AdminRefusal),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransferRecord {
    pub transfer_id: TransferId,
    pub project: ProjectId,
    pub kind: TransferKind,
    pub requester: String,
    pub reason: String,
    pub old_epoch: Epoch,
    pub to_installation: InstallationId,
    pub state: TransferState,
    pub request_hash: String,
    pub fleet_cleanup: FleetCleanup,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum FleetCleanup {
    Acknowledged,
    Owed(ClaimCleanupDebt),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ClaimCleanupDebt {
    Granted {
        project: ProjectId,
        attempt: RequestId,
    },
    Refused {
        project: ProjectId,
        attempt: RequestId,
        call_id: ObjectCallId,
        retry_at: crate::RegistryTime,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredPreview {
    pub project: ProjectId,
    pub holder: InstallationId,
    pub epoch: Epoch,
    pub copy_fence: Epoch,
    pub acked: ReplicationPosition,
    pub schema: SchemaIdentity,
    pub last_seen: DateTime<Utc>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub(crate) enum TransferReply {
    Claim(ObjectCallId),
    Admin(ObjectCallId),
    Release(ObjectCallId),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub(crate) enum PendingContinuation {
    TailVerification(crate::session_tail_report::PendingTailReport),
    ClaimReservation {
        reply_id: ObjectCallId,
        transfer_id: TransferId,
    },
    ClaimCleanup {
        transfer_id: TransferId,
    },
    ImportProject {
        reply_id: ObjectCallId,
        project: ProjectId,
    },
    ClaimSchema {
        reply_id: ObjectCallId,
        attempt: collaboration_replication_protocol::RequestId,
        unheld: crate::UnheldProject,
        claimant: InstallationId,
    },
    ClaimRestore {
        reply_id: ObjectCallId,
        transfer_id: TransferId,
    },
    RejectedPreview {
        reply_id: ObjectCallId,
        project: ProjectId,
    },
    ReleaseSchemaUnknown {
        reply: TransferReply,
        transfer_id: TransferId,
    },
    Reserve {
        reply_id: ObjectCallId,
        lease: RegistryLease,
    },
    Preview {
        reply_id: ObjectCallId,
        lease: RegistryLease,
    },
    FenceTransfer {
        reply: TransferReply,
        transfer_id: TransferId,
    },
    SessionRegister {
        connection: ConnectionId,
        session_id: SessionId,
    },
    WelcomeCopy {
        connection: ConnectionId,
        session_id: SessionId,
        lease: LeaseStatus,
        remaining: Vec<LeaseStatus>,
        complete: Vec<collaboration_replication_protocol::ProjectSessionState>,
    },
    FrameAnswer {
        connection: ConnectionId,
        frame_id: collaboration_replication_protocol::FrameId,
        expected: crate::session_reply::SessionReplyKind,
    },
    FleetReservation {
        connection: ConnectionId,
        frame_id: collaboration_replication_protocol::FrameId,
        answer: collaboration_replication_protocol::SessionAnswer,
    },
}
