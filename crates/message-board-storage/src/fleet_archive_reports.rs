//! Automated reports share admission rules with central and never enter the ledger.
use crate::{FleetStorageError, fleet_decision_writes, fleet_record_codec::read_fleet_record};
use collaboration_replication_protocol::{Epoch, ReplicationPosition};
use fleet_admission::{
    ArchiveFinalization, ArchiveReport, ArchiveReportOutcome, FleetFeedEntry, FleetReadKey,
    FleetReads, FleetSnapshot, decide_archive_report,
};
use message_board::*;
use sqlx_turso::TursoConnection;

pub(crate) async fn reports(
    connection: &mut TursoConnection,
    board: &BoardId,
) -> Result<Vec<ArchiveReport>, FleetStorageError> {
    let board_id = board.as_str();
    let rows = sqlx_turso::query!(r#"SELECT board AS "board!: String", project AS "project!: String", position_epoch AS "position_epoch!: String", position_commit AS "position_commit!: String", installation AS "installation!: String", reported_at AS "reported_at!: String" FROM archive_reports WHERE board = ? ORDER BY project"#, board_id).fetch_all(connection).await?;
    rows.into_iter()
        .map(|row| {
            let invalid = || FleetStorageError::InvalidRecord;
            let stored_board = BoardId::try_from(row.board).map_err(|_| invalid())?;
            if stored_board != *board {
                return Err(invalid());
            }
            Ok(ArchiveReport {
                board: stored_board,
                project: ProjectId::try_from(row.project).map_err(|_| invalid())?,
                position: ReplicationPosition::new(
                    Epoch::new(row.position_epoch.parse().map_err(|_| invalid())?)
                        .map_err(|_| invalid())?,
                    row.position_commit
                        .parse::<u64>()
                        .map_err(|_| invalid())?
                        .into(),
                ),
                installation: ServiceId::try_from(row.installation).map_err(|_| invalid())?,
                reported_at: row.reported_at.parse().map_err(|_| invalid())?,
            })
        })
        .collect()
}

async fn snapshot(
    connection: &mut TursoConnection,
    report: &ArchiveReport,
) -> Result<FleetSnapshot, FleetStorageError> {
    let reference = report.board.clone().into();
    let Some(Record::Board(board)) = read_fleet_record(connection, &reference).await? else {
        return Ok(FleetSnapshot::empty());
    };
    crate::fleet_snapshot_reads::read_keys(
        connection,
        FleetReads {
            keys: vec![
                FleetReadKey::FeedHighWater {},
                FleetReadKey::Record { record: reference },
                FleetReadKey::Record {
                    record: board.workspace_id.clone().into(),
                },
                FleetReadKey::Record {
                    record: report.project.clone().into(),
                },
                FleetReadKey::WorkspaceBoards {
                    workspace: board.workspace_id,
                },
                FleetReadKey::BoardProjects {
                    board: report.board.clone(),
                },
                FleetReadKey::ArchiveReports {
                    board: report.board.clone(),
                },
            ],
        },
    )
    .await
}

pub(crate) async fn receive(
    connection: &mut TursoConnection,
    report: &ArchiveReport,
) -> Result<ArchiveReportOutcome, FleetStorageError> {
    let snapshot = snapshot(connection, report).await?;
    let decision = match decide_archive_report(report, &snapshot) {
        Err(refusal) => return Ok(ArchiveReportOutcome::Refused(refusal)),
        Ok(decision) if !decision.record => return Ok(ArchiveReportOutcome::AlreadyRecorded),
        Ok(decision) => decision,
    };
    let board = report.board.as_str();
    let project = report.project.as_str();
    let position_epoch = report.position.epoch.value().to_string();
    let position_commit = report.position.commit_id.value().to_string();
    let installation = report.installation.as_str();
    let reported_at = report.reported_at.to_rfc3339();
    sqlx_turso::query!("INSERT INTO archive_reports(board, project, position_epoch, position_commit, installation, reported_at) VALUES (?, ?, ?, ?, ?, ?)", board, project, position_epoch, position_commit, installation, reported_at).execute(&mut *connection).await?;
    if let Some(finalization) = decision.finalize {
        finalize(connection, &snapshot, &finalization, report.reported_at).await?;
    }
    Ok(ArchiveReportOutcome::Recorded)
}

async fn finalize(
    connection: &mut TursoConnection,
    snapshot: &FleetSnapshot,
    finalization: &ArchiveFinalization,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), FleetStorageError> {
    let Some(Record::Board(board)) = snapshot.record(&finalization.board.clone().into()) else {
        return Err(FleetStorageError::InvalidRecord);
    };
    let mut board = board.clone();
    board.state = BoardState::Archived;
    let workspace_id = board.workspace_id.clone();
    let mut records = vec![Record::Board(board)];
    if finalization.workspace {
        let Some(Record::Workspace(workspace)) = snapshot.record(&workspace_id.into()) else {
            return Err(FleetStorageError::InvalidRecord);
        };
        let mut workspace = workspace.clone();
        workspace.state = WorkspaceState::Archived;
        records.push(Record::Workspace(workspace));
    }
    let mut position = snapshot.high_water;
    let mut entries = Vec::new();
    for mut record in records {
        position = position
            .successor()
            .map_err(|_| FleetStorageError::InvalidRecord)?;
        let envelope = record.envelope_mut();
        envelope.position = RecordPosition::Fleet(position);
        envelope.request = None;
        envelope.router_time = now;
        fleet_decision_writes::put_record(connection, &record).await?;
        entries.push(FleetFeedEntry { position, record });
    }
    fleet_decision_writes::append_feed(connection, &entries).await
}
