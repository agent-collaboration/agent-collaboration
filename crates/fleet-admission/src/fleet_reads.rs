use crate::{FleetOperation, FleetRequest};
use message_board::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "scope",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum FleetNameScope {
    Workspaces {},
    WorkspaceBoards(WorkspaceId),
    BoardProjects(BoardId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FleetReadKey {
    ImportDigest {
        record: RecordRef,
    },
    Ledger {
        request: RequestId,
    },
    Record {
        record: RecordRef,
    },
    Name {
        scope: FleetNameScope,
        name: ResourceName,
    },
    Definition {
        workspace: WorkspaceId,
        name: ResourceName,
        version: ResourceName,
    },
    WorkspaceBoards {
        workspace: WorkspaceId,
    },
    WorkspaceArchiveInputs {
        workspace: WorkspaceId,
    },
    BoardProjects {
        board: BoardId,
    },
    ArchiveReports {
        board: BoardId,
    },
    Repositories {
        workspace: WorkspaceId,
    },
    FeedHighWater {},
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetReads {
    pub keys: Vec<FleetReadKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetNameOwner {
    pub scope: FleetNameScope,
    pub name: ResourceName,
    pub record: RecordRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetSnapshot {
    pub imports: Vec<crate::ImportedFleetRecord>,
    pub ledger: Option<RequestLedgerEntry>,
    pub records: Vec<Record>,
    pub names: Vec<FleetNameOwner>,
    pub definitions: Vec<ProcessDefinition>,
    pub workspace_boards: Vec<Board>,
    pub board_projects: Vec<ProjectDirectoryEntry>,
    pub archive_reports: Vec<ProjectId>,
    pub unheld_projects: Vec<ProjectId>,
    pub claim_reservations: Vec<ProjectId>,
    pub repositories: Vec<RepositoryRef>,
    pub high_water: FleetPosition,
}

impl FleetSnapshot {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            imports: Vec::new(),
            ledger: None,
            records: Vec::new(),
            names: Vec::new(),
            definitions: Vec::new(),
            workspace_boards: Vec::new(),
            board_projects: Vec::new(),
            archive_reports: Vec::new(),
            unheld_projects: Vec::new(),
            claim_reservations: Vec::new(),
            repositories: Vec::new(),
            high_water: FleetPosition::ZERO,
        }
    }
    #[must_use]
    pub fn record(&self, reference: &RecordRef) -> Option<&Record> {
        self.records
            .iter()
            .find(|record| record.reference() == *reference)
    }
}

#[must_use]
pub fn reads_for(request: &FleetRequest) -> FleetReads {
    let mut keys = vec![
        FleetReadKey::Ledger {
            request: request.request_id.clone(),
        },
        FleetReadKey::FeedHighWater {},
        FleetReadKey::Record {
            record: request.workspace.clone().into(),
        },
    ];
    let operation = &request.operation;
    match operation {
        FleetOperation::WorkspaceCreate { name, .. }
        | FleetOperation::WorkspaceUpdate { name, .. } => keys.push(FleetReadKey::Name {
            scope: FleetNameScope::Workspaces {},
            name: name.clone(),
        }),
        FleetOperation::BoardCreate { board, name, .. }
        | FleetOperation::BoardUpdate { board, name, .. } => {
            keys.push(FleetReadKey::Record {
                record: board.clone().into(),
            });
            keys.push(FleetReadKey::Name {
                scope: FleetNameScope::WorkspaceBoards(request.workspace.clone()),
                name: name.clone(),
            });
        }
        FleetOperation::WorkspaceArchive {} => keys.push(FleetReadKey::WorkspaceArchiveInputs {
            workspace: request.workspace.clone(),
        }),
        FleetOperation::BoardClose { board } => keys.push(FleetReadKey::Record {
            record: board.clone().into(),
        }),
        FleetOperation::BoardArchive { board } => {
            keys.push(FleetReadKey::Record {
                record: board.clone().into(),
            });
            keys.push(FleetReadKey::BoardProjects {
                board: board.clone(),
            });
            keys.push(FleetReadKey::ArchiveReports {
                board: board.clone(),
            });
        }
        FleetOperation::ProjectDirectoryCreate {
            project,
            board,
            title,
        }
        | FleetOperation::ProjectDirectoryUpdate {
            project,
            board,
            title,
        } => {
            keys.push(FleetReadKey::Record {
                record: project.clone().into(),
            });
            keys.push(FleetReadKey::Record {
                record: board.clone().into(),
            });
            keys.push(FleetReadKey::Name {
                scope: FleetNameScope::BoardProjects(board.clone()),
                name: title.clone(),
            });
        }
        FleetOperation::AdrCreate {
            adr, supersedes, ..
        } => {
            keys.push(FleetReadKey::Record {
                record: adr.clone().into(),
            });
            if let Some(previous) = supersedes {
                keys.push(FleetReadKey::Record {
                    record: previous.clone().into(),
                });
            }
        }
        FleetOperation::AdrTransition {
            adr, transition, ..
        } => {
            keys.push(FleetReadKey::Record {
                record: adr.clone().into(),
            });
            if let AdrTransition::Supersede { by } = transition {
                keys.push(FleetReadKey::Record {
                    record: by.clone().into(),
                });
            }
        }
        FleetOperation::DefinitionRegister { definition } => keys.push(FleetReadKey::Definition {
            workspace: request.workspace.clone(),
            name: definition.reference.name.clone(),
            version: definition.reference.version.clone(),
        }),
        FleetOperation::RepositoryAttach { .. } | FleetOperation::RepositoryDetach { .. } => keys
            .push(FleetReadKey::Repositories {
                workspace: request.workspace.clone(),
            }),
    }
    FleetReads { keys }
}
