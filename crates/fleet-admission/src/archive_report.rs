use crate::FleetSnapshot;
use chrono::{DateTime, Utc};
use fleet_sync_protocol::ReplicationPosition;
use message_board::{BoardId, BoardState, ProjectId, Record, RecordRef, ServiceId, WorkspaceState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArchiveReport {
    pub board: BoardId,
    pub project: ProjectId,
    pub position: ReplicationPosition,
    pub installation: ServiceId,
    pub reported_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ArchiveReportOutcome {
    Recorded,
    AlreadyRecorded,
    Refused(ArchiveReportRefusal),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ArchiveReportRefusal {
    NotFound(RecordRef),
    ProjectNotOnBoard { project: ProjectId, board: BoardId },
    BoardNotArchiving { state: BoardState },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveFinalization {
    pub board: BoardId,
    pub workspace: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveReportDecision {
    pub record: bool,
    pub finalize: Option<ArchiveFinalization>,
}

pub fn decide_archive_report(
    report: &ArchiveReport,
    snapshot: &FleetSnapshot,
) -> Result<ArchiveReportDecision, ArchiveReportRefusal> {
    let board_reference = report.board.clone().into();
    let Some(Record::Board(board)) = snapshot.record(&board_reference) else {
        return Err(ArchiveReportRefusal::NotFound(board_reference));
    };
    let workspace_reference = board.workspace_id.clone().into();
    let Some(Record::Workspace(_)) = snapshot.record(&workspace_reference) else {
        return Err(ArchiveReportRefusal::NotFound(workspace_reference));
    };
    if !snapshot
        .board_projects
        .iter()
        .any(|project| project.id == report.project && project.board == report.board)
    {
        return Err(
            if snapshot.record(&report.project.clone().into()).is_some() {
                ArchiveReportRefusal::ProjectNotOnBoard {
                    project: report.project.clone(),
                    board: report.board.clone(),
                }
            } else {
                ArchiveReportRefusal::NotFound(report.project.clone().into())
            },
        );
    }
    // The persisted pair is the identity. In particular a successor's resend
    // cannot replace its position, reporter or first acknowledgement.
    if snapshot.archive_reports.contains(&report.project) {
        return Ok(ArchiveReportDecision {
            record: false,
            finalize: None,
        });
    }
    if board.state != BoardState::Archiving {
        return Err(ArchiveReportRefusal::BoardNotArchiving { state: board.state });
    }
    Ok(ArchiveReportDecision {
        record: true,
        finalize: finalization_after(&report.board, snapshot, Some(&report.project)),
    })
}

/// Reportless completion uses the same predicate after reservation settlement
/// or an Unheld projection; no synthetic holder report is needed.
#[must_use]
pub fn decide_archive_finalization(
    board: &BoardId,
    snapshot: &FleetSnapshot,
) -> Option<ArchiveFinalization> {
    finalization_after(board, snapshot, None)
}

fn finalization_after(
    board: &BoardId,
    snapshot: &FleetSnapshot,
    reported: Option<&ProjectId>,
) -> Option<ArchiveFinalization> {
    let Record::Board(board_record) = snapshot.record(&board.clone().into())? else {
        return None;
    };
    if board_record.state != BoardState::Archiving {
        return None;
    }
    let Record::Workspace(workspace) =
        snapshot.record(&board_record.workspace_id.clone().into())?
    else {
        return None;
    };
    let complete = snapshot
        .board_projects
        .iter()
        .filter(|project| project.board == *board)
        .all(|project| {
            reported == Some(&project.id)
                || snapshot.archive_reports.contains(&project.id)
                || (snapshot.unheld_projects.contains(&project.id)
                    && !snapshot.claim_reservations.contains(&project.id))
        });
    complete.then(|| ArchiveFinalization {
        board: board.clone(),
        workspace: workspace.state == WorkspaceState::Archiving
            && snapshot
                .workspace_boards
                .iter()
                .all(|other| other.board_id == *board || other.state == BoardState::Archived),
    })
}
