use fleet_admission::FleetImportBundle;
use fleet_sync_protocol::{ImportDigest, SnapshotManifest};
use message_board::{
    BoardId, Description, FeedCursor, HumanId, ProjectId, RecordId, ResourceName, WorkspaceId,
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NameCollision {
    #[serde(flatten)]
    pub location: CollisionLocation,
    pub name: ResourceName,
    pub ids: Vec<RecordId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CollisionLocation {
    Workspace,
    Board { scope: WorkspaceId },
    Project { scope: BoardId },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectImportReport {
    pub project: ProjectId,
    pub workspace: WorkspaceId,
    pub board: BoardId,
    pub title: ResourceName,
    pub description: Description,
    pub posts: u64,
    pub feed_rows: u64,
    pub imported_resolutions: u64,
    pub old_to_new: BTreeMap<u64, FeedCursor>,
    pub digest: ImportDigest,
    pub manifest: SnapshotManifest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeReport {
    pub owner: HumanId,
    pub fleet: FleetImportBundle,
    pub fleet_digest: fleet_admission::FleetImportDigest,
    pub collisions: Vec<NameCollision>,
    pub projects: Vec<ProjectImportReport>,
    pub workspace_counts: Vec<WorkspaceImportCounts>,
    pub board_counts: Vec<BoardImportCounts>,
    pub skipped: Vec<ProjectId>,
    pub refused: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceImportCounts {
    pub workspace: WorkspaceId,
    pub boards: usize,
    pub projects: usize,
    pub posts: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardImportCounts {
    pub board: BoardId,
    pub workspace: WorkspaceId,
    pub projects: usize,
    pub posts: u64,
}

pub(crate) fn resource_counts(
    fleet: &FleetImportBundle,
    projects: &[ProjectImportReport],
) -> (Vec<WorkspaceImportCounts>, Vec<BoardImportCounts>) {
    let mut boards = Vec::new();
    let mut workspaces = Vec::new();
    for record in &fleet.records {
        match record {
            fleet_admission::FleetImportRecord::Workspace { id, .. } => {
                let project_count = projects
                    .iter()
                    .filter(|project| &project.workspace == id)
                    .count();
                let posts = projects
                    .iter()
                    .filter(|project| &project.workspace == id)
                    .map(|project| project.posts)
                    .sum();
                let board_count = fleet.records.iter().filter(|record| {
                    matches!(record, fleet_admission::FleetImportRecord::Board { workspace, .. } if workspace == id)
                }).count();
                workspaces.push(WorkspaceImportCounts {
                    workspace: id.clone(),
                    boards: board_count,
                    projects: project_count,
                    posts,
                });
            }
            fleet_admission::FleetImportRecord::Board { id, workspace, .. } => {
                boards.push(BoardImportCounts {
                    board: id.clone(),
                    workspace: workspace.clone(),
                    projects: projects
                        .iter()
                        .filter(|project| &project.board == id)
                        .count(),
                    posts: projects
                        .iter()
                        .filter(|project| &project.board == id)
                        .map(|project| project.posts)
                        .sum(),
                });
            }
            fleet_admission::FleetImportRecord::Directory { .. } => {}
        }
    }
    (workspaces, boards)
}
