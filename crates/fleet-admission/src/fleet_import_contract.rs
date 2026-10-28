use crate::{FleetAdmissionError, FleetFeedEntry};
use message_board::{
    BoardId, CanonicalHash, Description, HumanId, MachineLabel, ProjectId, Record, RecordRef,
    RecordTime, RepositoryRef, ResourceName, ServiceId, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FleetImportDigest(CanonicalHash);
impl FleetImportDigest {
    pub fn new(value: String) -> Result<Self, FleetAdmissionError> {
        CanonicalHash::try_from(value)
            .map(Self)
            .map_err(|_| FleetAdmissionError::InvalidSnapshot)
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

fn digest_content<TContent: Serialize>(
    content: &TContent,
) -> Result<FleetImportDigest, FleetAdmissionError> {
    FleetImportDigest::new(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(content)?)
    ))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportProvenance {
    pub installation: ServiceId,
    pub machine: MachineLabel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportedBoardState {
    Active,
    Archived,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetImportRecord {
    Workspace {
        id: WorkspaceId,
        name: ResourceName,
        description: Description,
        repositories: Vec<RepositoryRef>,
        time: RecordTime,
    },
    Board {
        id: BoardId,
        workspace: WorkspaceId,
        name: ResourceName,
        description: Description,
        state: ImportedBoardState,
        time: RecordTime,
    },
    Directory {
        project: ProjectId,
        board: BoardId,
        title: ResourceName,
        time: RecordTime,
    },
}
impl FleetImportRecord {
    #[must_use]
    pub fn reference(&self) -> RecordRef {
        match self {
            Self::Workspace { id, .. } => id.clone().into(),
            Self::Board { id, .. } => id.clone().into(),
            Self::Directory { project, .. } => project.clone().into(),
        }
    }
    pub fn digest(&self) -> Result<FleetImportDigest, FleetAdmissionError> {
        digest_content(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetImportBundle {
    pub provenance: ImportProvenance,
    pub declared_owner: HumanId,
    pub records: Vec<FleetImportRecord>,
}
impl FleetImportBundle {
    /// Diagnostic identity only; provenance and arrival order are not content.
    pub fn digest(&self) -> Result<FleetImportDigest, FleetAdmissionError> {
        let mut digests = self
            .records
            .iter()
            .map(FleetImportRecord::digest)
            .collect::<Result<Vec<_>, _>>()?;
        digests.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        digest_content(&digests)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedFleetRecord {
    pub record: RecordRef,
    pub digest: FleetImportDigest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ExistingFleetImport {
    Imported(FleetImportDigest),
    NotImported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetImportRefusal {
    ImportConflict { existing: ExistingFleetImport },
    ParentMissing { parent: RecordRef },
    NameConflict { existing: RecordRef },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetImportConflict {
    pub record: RecordRef,
    pub reason: FleetImportRefusal,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetImportCounts {
    pub workspaces: u64,
    pub boards: u64,
    pub directory_entries: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FleetImportResult {
    Imported { counts: FleetImportCounts },
    AlreadyImported,
    Refused { conflicts: Vec<FleetImportConflict> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetImportWrite {
    pub record: Record,
    pub digest: FleetImportDigest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetImportDecision {
    pub result: FleetImportResult,
    pub writes: Vec<FleetImportWrite>,
    pub feed: Vec<FleetFeedEntry>,
}
