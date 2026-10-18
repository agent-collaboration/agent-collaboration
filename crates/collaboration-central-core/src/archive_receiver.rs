//! Fleet-owned archive bookkeeping and atomic finalizing writes.
use crate::copy_storage::{query_statement, statement, text};
use crate::{CentralError, RegistryTime, SqlTx};
use collaboration_replication_protocol::{FleetPayload, SessionAnswer};
use fleet_admission::*;
use message_board::*;

pub(crate) fn reports(
    tx: &mut dyn SqlTx,
    board: &BoardId,
) -> Result<Vec<ArchiveReport>, CentralError> {
    tx.query(query_statement(
        "SELECT board,project,position_epoch,position_commit,installation,reported_at FROM archive_reports WHERE board=? ORDER BY project",
        vec![text(board.as_str())],
        ["board", "project", "position_epoch", "position_commit", "installation", "reported_at"],
    ))?.iter().map(|row| {
        let invalid = || crate::fleet_storage::corrupt("archive report");
        let stored_board = BoardId::try_from(row.text("board")?.to_owned()).map_err(|_| invalid())?;
        if stored_board != *board { return Err(invalid()); }
        Ok(ArchiveReport {
            board: stored_board,
            project: ProjectId::try_from(row.text("project")?.to_owned()).map_err(|_| invalid())?,
            position: collaboration_replication_protocol::ReplicationPosition::new(
                collaboration_replication_protocol::Epoch::new(row.text("position_epoch")?.parse().map_err(|_| invalid())?).map_err(|_| invalid())?,
                row.text("position_commit")?.parse::<u64>().map_err(|_| invalid())?.into(),
            ),
            installation: ServiceId::try_from(row.text("installation")?.to_owned()).map_err(|_| invalid())?,
            reported_at: row.text("reported_at")?.parse().map_err(|_| invalid())?,
        })
    }).collect()
}

fn snapshot(
    tx: &mut dyn SqlTx,
    board: &BoardId,
    project: Option<&ProjectId>,
) -> Result<FleetSnapshot, CentralError> {
    let reference = board.clone().into();
    let Some(Record::Board(board_record)) = crate::fleet_storage::read_record(tx, &reference)?
    else {
        return Ok(FleetSnapshot::empty());
    };
    let mut keys = vec![
        FleetReadKey::Record { record: reference },
        FleetReadKey::Record {
            record: board_record.workspace_id.clone().into(),
        },
        FleetReadKey::WorkspaceBoards {
            workspace: board_record.workspace_id,
        },
        FleetReadKey::BoardProjects {
            board: board.clone(),
        },
        FleetReadKey::ArchiveReports {
            board: board.clone(),
        },
    ];
    if let Some(project) = project {
        keys.push(FleetReadKey::Record {
            record: project.clone().into(),
        });
    }
    crate::fleet_snapshot::read_snapshot(tx, FleetReads { keys })
}

pub(crate) fn receive(
    tx: &mut dyn SqlTx,
    report: ArchiveReport,
) -> Result<SessionAnswer, CentralError> {
    let snapshot = snapshot(tx, &report.board, Some(&report.project))?;
    let outcome = match decide_archive_report(&report, &snapshot) {
        Err(refusal) => ArchiveReportOutcome::Refused(refusal),
        Ok(decision) if !decision.record => ArchiveReportOutcome::AlreadyRecorded,
        Ok(decision) => {
            tx.execute(statement(
                "INSERT INTO archive_reports(board,project,position_epoch,position_commit,installation,reported_at) VALUES(?,?,?,?,?,?)",
                vec![text(report.board.as_str()), text(report.project.as_str()), text(report.position.epoch.value().to_string()), text(report.position.commit_id.value().to_string()), text(report.installation.as_str()), text(report.reported_at.to_rfc3339())],
            ))?;
            if let Some(finalization) = decision.finalize {
                apply(
                    tx,
                    &snapshot,
                    &finalization,
                    RegistryTime(report.reported_at),
                )?;
            }
            ArchiveReportOutcome::Recorded
        }
    };
    Ok(SessionAnswer::ArchiveReport(FleetPayload::new(
        serde_json::to_value(outcome).map_err(|error| CentralError::Encoding {
            message: error.to_string(),
        })?,
    )))
}

fn apply(
    tx: &mut dyn SqlTx,
    snapshot: &FleetSnapshot,
    finalization: &ArchiveFinalization,
    now: RegistryTime,
) -> Result<(), CentralError> {
    let Some(Record::Board(board)) = snapshot.record(&finalization.board.clone().into()) else {
        return Err(crate::fleet_storage::corrupt("archive board"));
    };
    let mut board = board.clone();
    board.state = BoardState::Archived;
    let mut records = vec![Record::Board(board.clone())];
    if finalization.workspace {
        let Some(Record::Workspace(workspace)) = snapshot.record(&board.workspace_id.into()) else {
            return Err(crate::fleet_storage::corrupt("archive workspace"));
        };
        let mut workspace = workspace.clone();
        workspace.state = WorkspaceState::Archived;
        records.push(Record::Workspace(workspace));
    }
    let mut position = crate::fleet_storage::checkpoint(tx)?;
    let mut entries = Vec::new();
    for mut record in records {
        position = position
            .successor()
            .map_err(|_| crate::fleet_storage::corrupt("archive feed position"))?;
        let envelope = record.envelope_mut();
        envelope.position = RecordPosition::Fleet(position);
        envelope.request = None;
        envelope.router_time = now.0;
        crate::fleet_storage::put_record(tx, &record, None)?;
        entries.push(FleetFeedEntry { position, record });
    }
    crate::fleet_feed::append(tx, &entries, now)
}

pub(crate) fn complete_board(
    tx: &mut dyn SqlTx,
    board: &BoardId,
    now: RegistryTime,
) -> Result<(), CentralError> {
    let snapshot = snapshot(tx, board, None)?;
    if let Some(finalization) = decide_archive_finalization(board, &snapshot) {
        apply(tx, &snapshot, &finalization, now)?;
    }
    Ok(())
}

pub(crate) fn complete_project(
    tx: &mut dyn SqlTx,
    project: &collaboration_replication_protocol::ProjectId,
    now: RegistryTime,
) -> Result<(), CentralError> {
    let project = ProjectId::try_from(project.as_str().to_owned())
        .map_err(|_| crate::fleet_storage::corrupt("archive project"))?;
    if let Some(Record::DirectoryEntry(entry)) =
        crate::fleet_storage::read_record(tx, &project.into())?
    {
        complete_board(tx, &entry.board, now)?;
    }
    Ok(())
}

pub(crate) fn complete_request(
    tx: &mut dyn SqlTx,
    request: &FleetRequest,
    now: RegistryTime,
) -> Result<(), CentralError> {
    match &request.operation {
        FleetOperation::BoardArchive { board } => complete_board(tx, board, now),
        FleetOperation::WorkspaceArchive {} => {
            let snapshot = crate::fleet_snapshot::read_snapshot(
                tx,
                FleetReads {
                    keys: vec![FleetReadKey::WorkspaceBoards {
                        workspace: request.workspace.clone(),
                    }],
                },
            )?;
            for board in snapshot.workspace_boards {
                complete_board(tx, &board.board_id, now)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
