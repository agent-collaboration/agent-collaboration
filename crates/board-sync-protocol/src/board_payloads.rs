use fleet_sync_protocol::{Epoch, PeerId, ReplicationPosition};
use message_board::{ActorOrigin, BoardId, ProjectId, RecordTime, RequestId};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// An opaque board-owned body. Its receiving owner decodes its existing contract.
/// Central forwards posts and answers without interpreting their contents.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BoardPayload(serde_json::Value);

impl BoardPayload {
    pub fn encode(value: &impl Serialize) -> Result<Self, serde_json::Error> {
        serde_json::to_value(value).map(Self)
    }

    pub fn decode<T: DeserializeOwned>(self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ForwardedPostKind {
    ThreadCreate,
    ThreadPost,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForwardedPost {
    pub kind: ForwardedPostKind,
    pub body: BoardPayload,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetRequestEnvelope {
    pub request_id: RequestId,
    pub operation: BoardPayload,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BoardAnnotation {
    ProjectSummary { latest_record_at: RecordTime },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BoardReadScope {
    InstallationPeer {
        service: message_board::ServiceId,
    },
    InstallationPeers {
        after: Option<PeerId>,
    },
    RequestEffects {
        workspace: message_board::WorkspaceId,
        request: RequestId,
        after: Option<message_board::RequestEffectsCursor>,
    },
    FleetRecords,
    FleetSnapshot,
    Directory {
        query: BoardPayload,
    },
    Notices,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum BoardReadCursor {
    Fleet(message_board::FleetPosition),
    FleetSnapshot(crate::FleetSnapshotCursor),
    Notices(fleet_sync_protocol::NoticePosition),
    Snapshot(fleet_sync_protocol::SnapshotCursor),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BoardNotice {
    AutomationBatchReady {
        batch_id: agent_automation::BatchId,
    },
    MailWaiting {
        stream: fleet_sync_protocol::DeliveryStream,
    },
    FleetFeedAdvanced {
        through: message_board::FleetPosition,
    },
    LeaseSuperseded {
        project: ProjectId,
        epoch: Epoch,
    },
    LeaseGranted {
        project: ProjectId,
        epoch: Epoch,
    },
    BoardArchiving {
        board: BoardId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ForwardOutcome {
    Answered {
        reply: BoardPayload,
    },
    SenderHolds,
    NotHeld,
    HolderUnreachable {
        last_seen: Option<fleet_sync_protocol::SyncTime>,
    },
    Unanswered,
    Overloaded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerifiedForwardedPost {
    pub project: ProjectId,
    pub origin: ActorOrigin,
    pub post: ForwardedPost,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArchiveSeen {
    pub board: BoardId,
    pub project: ProjectId,
    pub position: ReplicationPosition,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BoardReadPage {
    InstallationPeer(InstallationPeerLookup),
    InstallationPeers(InstallationPeerPage),
    RequestEffects {
        result: message_board::RequestEffectsRead,
        as_of: chrono::DateTime<chrono::Utc>,
    },
    FleetFeed(crate::FleetFeedRead),
    FleetSnapshot {
        records: Vec<BoardPayload>,
        next: Option<crate::FleetSnapshotCursor>,
        snapshot_at: message_board::FleetPosition,
        as_of: chrono::DateTime<chrono::Utc>,
    },
    Directory {
        entries: Vec<BoardPayload>,
        as_of: chrono::DateTime<chrono::Utc>,
    },
    Notices(NoticeRead),
}

/// The registry's immutable identity binding; revocation is checked at submission.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum InstallationPeerLookup {
    Enrolled { peer: PeerId },
    Unknown,
}

/// A full compact BoardConsumerAnswer with a continuation is 80,118 bytes:
/// each canonical service/peer identity is 36 ASCII bytes. No request limit is needed.
pub const MAX_INSTALLATION_PEERS_PAGE_ROWS: usize = 1_000;

/// Immutable installation bindings in strictly increasing peer order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "InstallationPeerPageFields")]
pub struct InstallationPeerPage {
    peers: Vec<(message_board::ServiceId, PeerId)>,
    next: Option<PeerId>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallationPeerPageFields {
    peers: Vec<(message_board::ServiceId, PeerId)>,
    next: Option<PeerId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallationPeerPageError {
    TooManyPeers,
    UnorderedPeers,
    DuplicateServices,
    InvalidContinuation,
    CursorNotAdvanced,
}

impl std::fmt::Display for InstallationPeerPageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::TooManyPeers => "installation peer page exceeds its row limit",
            Self::UnorderedPeers => "installation peer page is not strictly ordered",
            Self::DuplicateServices => "installation peer page repeats a service identity",
            Self::InvalidContinuation => "installation peer page has an invalid continuation",
            Self::CursorNotAdvanced => "installation peer page does not advance its cursor",
        })
    }
}
impl std::error::Error for InstallationPeerPageError {}

impl InstallationPeerPage {
    pub fn new(
        peers: Vec<(message_board::ServiceId, PeerId)>,
        next: Option<PeerId>,
    ) -> Result<Self, InstallationPeerPageError> {
        if peers.len() > MAX_INSTALLATION_PEERS_PAGE_ROWS {
            return Err(InstallationPeerPageError::TooManyPeers);
        }
        if peers
            .iter()
            .zip(peers.iter().skip(1))
            .any(|(previous, next)| previous.1 >= next.1)
        {
            return Err(InstallationPeerPageError::UnorderedPeers);
        }
        let services: std::collections::BTreeSet<_> = peers.iter().map(|pair| &pair.0).collect();
        if services.len() != peers.len() {
            return Err(InstallationPeerPageError::DuplicateServices);
        }
        if let Some(next) = next
            && peers.last().map(|pair| pair.1) != Some(next)
        {
            return Err(InstallationPeerPageError::InvalidContinuation);
        }
        Ok(Self { peers, next })
    }

    #[must_use]
    pub fn peers(&self) -> &[(message_board::ServiceId, PeerId)] {
        &self.peers
    }

    #[must_use]
    pub fn next(&self) -> Option<PeerId> {
        self.next
    }

    /// The request supplies the lower bound; an answer cannot repeat a previous page.
    pub fn validate_after(&self, after: Option<PeerId>) -> Result<(), InstallationPeerPageError> {
        if after.is_some_and(|after| self.peers.first().is_some_and(|pair| pair.1 <= after)) {
            return Err(InstallationPeerPageError::CursorNotAdvanced);
        }
        Ok(())
    }
}

impl TryFrom<InstallationPeerPageFields> for InstallationPeerPage {
    type Error = InstallationPeerPageError;
    fn try_from(fields: InstallationPeerPageFields) -> Result<Self, Self::Error> {
        Self::new(fields.peers, fields.next)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PositionedBoardNotice {
    pub position: fleet_sync_protocol::NoticePosition,
    pub kind: BoardNotice,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum NoticeRead {
    Page {
        notices: Vec<PositionedBoardNotice>,
        as_of: chrono::DateTime<chrono::Utc>,
    },
    CursorExpired {
        /// Last pruned machine position; reading after this floor is valid.
        /// The first retained notice, when present, is floor plus one.
        floor: fleet_sync_protocol::NoticePosition,
    },
    CursorInvalid,
}
