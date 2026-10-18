//! Matching Fleet identity for project-import scenarios that exercise claims or reads.
use crate::{ConformanceClient, ConformanceResult, holder_fixtures::*};
use collaboration_replication_protocol::ProjectId;
use fleet_admission::{
    DirectoryEntry, DirectorySide, FleetImportBundle, FleetImportCounts, FleetImportRecord,
    FleetImportResult, ImportProvenance, ImportedBoardState,
};
use message_board::{BoardState, Description, MachineLabel, ResourceName, ServiceId};

pub(crate) async fn import_identity(
    client: &ConformanceClient,
    project: &ProjectId,
) -> ConformanceResult<DirectoryEntry> {
    let workspace = message_board::WorkspaceId::try_from(uuid::Uuid::now_v7().to_string())?;
    let board = message_board::BoardId::try_from(uuid::Uuid::now_v7().to_string())?;
    let name = ResourceName::try_from("Directory project".to_owned())?;
    let project = message_board::ProjectId::try_from(project.as_str().to_owned())?;
    equal(
        client
            .import_fleet(&FleetImportBundle {
                provenance: ImportProvenance {
                    installation: ServiceId::try_from(
                        "00000000-0000-4000-8000-000000000001".to_owned(),
                    )?,
                    machine: MachineLabel::try_from("directory original computer".to_owned())?,
                },
                records: vec![
                    FleetImportRecord::Workspace {
                        id: workspace.clone(),
                        name: ResourceName::try_from(format!(
                            "Directory workspace {}",
                            workspace.as_str()
                        ))?,
                        description: Description::try_from(
                            "Directory original workspace".to_owned(),
                        )?,
                        repositories: Vec::new(),
                        created_at: time()?.0,
                    },
                    FleetImportRecord::Board {
                        id: board.clone(),
                        workspace: workspace.clone(),
                        name: ResourceName::try_from("Directory board".to_owned())?,
                        description: Description::try_from("Directory original board".to_owned())?,
                        state: ImportedBoardState::Active,
                        created_at: time()?.0,
                    },
                    FleetImportRecord::Directory {
                        project: project.clone(),
                        board: board.clone(),
                        title: name.clone(),
                        created_at: time()?.0,
                    },
                ],
            })
            .await?,
        FleetImportResult::Imported {
            counts: FleetImportCounts {
                workspaces: 1,
                boards: 1,
                directory_entries: 1,
            },
        },
    )?;
    Ok(DirectoryEntry {
        project,
        name,
        workspace,
        board,
        board_archive: BoardState::Open,
        copy_side: DirectorySide::NotYetReported,
        lease_side: DirectorySide::NotYetReported,
    })
}
