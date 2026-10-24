use message_board::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetRequest {
    pub request_id: RequestId,
    pub workspace: WorkspaceId,
    pub actor: RecordedActor,
    pub acting_for: Option<ActingForIdentity>,
    pub protocol_version: ProtocolVersion,
    pub operation: FleetOperation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessDefinitionInput {
    pub reference: ProcessDefinitionRef,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetOperation {
    WorkspaceCreate {
        name: ResourceName,
        description: Description,
    },
    WorkspaceUpdate {
        name: ResourceName,
        description: Description,
    },
    WorkspaceArchive {},
    RepositoryAttach {
        repository: RepositoryRef,
    },
    RepositoryDetach {
        repository: RepositoryRef,
    },
    BoardCreate {
        board: BoardId,
        name: ResourceName,
        description: Description,
        board_kind: BoardKind,
    },
    BoardUpdate {
        board: BoardId,
        name: ResourceName,
        description: Description,
    },
    BoardClose {
        board: BoardId,
    },
    BoardArchive {
        board: BoardId,
    },
    ProjectDirectoryCreate {
        project: ProjectId,
        board: BoardId,
        title: ResourceName,
    },
    ProjectDirectoryUpdate {
        project: ProjectId,
        board: BoardId,
        title: ResourceName,
    },
    AdrCreate {
        adr: AdrId,
        title: ResourceName,
        text: MarkdownText,
        supersedes: Option<AdrId>,
        made_in: Option<RecordRef>,
        repository_files: Vec<ArtifactRef>,
    },
    AdrTransition {
        adr: AdrId,
        transition: AdrTransition,
        expected_revision: SubjectRevision,
    },
    DefinitionRegister {
        definition: ProcessDefinitionInput,
    },
}

pub fn canonical_hash(request: &FleetRequest) -> Result<CanonicalHash, crate::FleetAdmissionError> {
    // E10's actor identity includes a session's own origin, never the recording holder.
    let bytes = serde_json::to_vec(&(
        &request.workspace,
        &request.actor,
        &request.acting_for,
        request.protocol_version,
        &request.operation,
    ))?;
    CanonicalHash::try_from(format!("{:x}", Sha256::digest(bytes)))
        .map_err(|_| crate::FleetAdmissionError::InvalidSnapshot)
}
