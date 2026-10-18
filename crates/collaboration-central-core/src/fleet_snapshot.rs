use crate::copy_storage::{query_statement, text};
use crate::fleet_storage::{corrupt, kind_tag, read_record};
use crate::{CentralError, SqlTx};
use fleet_admission::*;
use message_board::*;

pub(crate) fn read_snapshot(
    tx: &mut dyn SqlTx,
    reads: FleetReads,
) -> Result<FleetSnapshot, CentralError> {
    let mut snapshot = FleetSnapshot::empty();
    for key in reads.keys {
        match key {
            FleetReadKey::FeedHighWater {} => {
                snapshot.high_water = crate::fleet_storage::checkpoint(tx)?
            }
            FleetReadKey::Record { record } => {
                if let Some(record) = read_record(tx, &record)?
                    && snapshot.record(&record.reference()).is_none()
                {
                    snapshot.records.push(record);
                }
            }
            FleetReadKey::ImportDigest { record } => {
                let rows = tx.query(query_statement(
                    "SELECT import_digest FROM fleet_records WHERE kind=? AND id=?",
                    vec![text(kind_tag(record.kind)?), text(record.id.as_str())],
                    ["import_digest"],
                ))?;
                if let Some(row) = rows.first() {
                    match row.value("import_digest")? {
                        collaboration_replication_protocol::SqlValue::Null => {}
                        collaboration_replication_protocol::SqlValue::Text(value) => {
                            snapshot.imports.push(ImportedFleetRecord {
                                record,
                                digest: FleetImportDigest::new(value.clone())
                                    .map_err(|_| corrupt("fleet import digest"))?,
                            })
                        }
                        _ => return Err(corrupt("fleet import digest")),
                    }
                }
            }
            FleetReadKey::Name { scope, name } => {
                if let Some(record) = name_owner(tx, &scope, &name)? {
                    snapshot.names.push(FleetNameOwner {
                        scope,
                        name,
                        record,
                    });
                }
            }
            FleetReadKey::Ledger { request } => {
                let rows = tx.query(query_statement(
                    "SELECT canonical_hash,payload FROM fleet_request_ledger WHERE request_id=?",
                    vec![text(request.as_str())],
                    ["canonical_hash", "payload"],
                ))?;
                if let Some(row) = rows.first() {
                    let ledger: RequestLedgerEntry = serde_json::from_str(row.text("payload")?)
                        .map_err(|_| corrupt("fleet ledger"))?;
                    if ledger.request_id != request
                        || ledger.canonical_hash.as_str() != row.text("canonical_hash")?
                    {
                        return Err(corrupt("fleet ledger"));
                    }
                    snapshot.ledger = Some(ledger);
                }
            }
            FleetReadKey::WorkspaceBoards { workspace } => {
                snapshot.workspace_boards = workspace_boards(tx, &workspace)?
            }
            FleetReadKey::WorkspaceArchiveInputs { workspace } => {
                snapshot.workspace_boards = workspace_boards(tx, &workspace)?;
                for board in &snapshot.workspace_boards {
                    snapshot
                        .board_projects
                        .extend(board_projects(tx, &board.board_id)?);
                    snapshot
                        .archive_reports
                        .extend(archive_reports(tx, &board.board_id)?);
                }
            }
            FleetReadKey::BoardProjects { board } => {
                snapshot.board_projects = board_projects(tx, &board)?
            }
            FleetReadKey::ArchiveReports { board } => {
                snapshot.archive_reports = archive_reports(tx, &board)?
            }
            FleetReadKey::Repositories { workspace } => {
                snapshot.repositories = tx.query(query_statement("SELECT reference FROM fleet_repositories WHERE workspace_id=? ORDER BY reference", vec![text(workspace.as_str())], ["reference"]))?.iter().map(|row| serde_json::from_str(row.text("reference")?).map_err(|_| corrupt("fleet repository"))).collect::<Result<_, _>>()?;
            }
            FleetReadKey::Definition {
                workspace,
                name,
                version,
            } => {
                let records = indexed_records(
                    tx,
                    "SELECT record_id AS id FROM fleet_definitions WHERE workspace_id=? AND name=? AND version=?",
                    vec![
                        text(workspace.as_str()),
                        text(name.as_str()),
                        text(version.as_str()),
                    ],
                    RecordKind::ProcessDefinition,
                )?;
                for record in records {
                    let Record::ProcessDefinition(record) = record else {
                        return Err(corrupt("fleet definition"));
                    };
                    snapshot.definitions.push(record);
                }
            }
        }
    }
    for project in &snapshot.board_projects {
        let wire_project = collaboration_replication_protocol::ProjectId::new(project.id.as_str())
            .map_err(|_| corrupt("archive project"))?;
        if matches!(
            crate::directory_receiver::lease_metadata(tx, &wire_project)?,
            DirectorySide::Known(LeaseMetadata {
                lease: collaboration_replication_protocol::DirectoryLease::Unheld,
                ..
            })
        ) {
            snapshot.unheld_projects.push(project.id.clone());
        }
        if crate::claim_reservation::active(tx, &wire_project)? {
            snapshot.claim_reservations.push(project.id.clone());
        }
    }
    Ok(snapshot)
}
fn indexed_records(
    tx: &mut dyn SqlTx,
    sql: &str,
    parameters: Vec<collaboration_replication_protocol::SqlValue>,
    kind: RecordKind,
) -> Result<Vec<Record>, CentralError> {
    let rows = tx.query(query_statement(sql, parameters, ["id"]))?;
    rows.iter()
        .map(|row| {
            let reference = RecordRef {
                kind,
                id: RecordId::try_from(row.text("id")?.to_owned())
                    .map_err(|_| corrupt("fleet index identity"))?,
            };
            read_record(tx, &reference)?.ok_or_else(|| corrupt("fleet index missing record"))
        })
        .collect()
}
fn workspace_boards(
    tx: &mut dyn SqlTx,
    workspace: &WorkspaceId,
) -> Result<Vec<Board>, CentralError> {
    indexed_records(
        tx,
        "SELECT board_id AS id FROM fleet_boards WHERE workspace_id=? ORDER BY board_id",
        vec![text(workspace.as_str())],
        RecordKind::Board,
    )?
    .into_iter()
    .map(|record| match record {
        Record::Board(record) => Ok(record),
        _ => Err(corrupt("fleet board")),
    })
    .collect()
}
fn board_projects(
    tx: &mut dyn SqlTx,
    board: &BoardId,
) -> Result<Vec<ProjectDirectoryEntry>, CentralError> {
    indexed_records(
        tx,
        "SELECT project_id AS id FROM fleet_directory WHERE board_id=? ORDER BY project_id",
        vec![text(board.as_str())],
        RecordKind::Project,
    )?
    .into_iter()
    .map(|record| match record {
        Record::DirectoryEntry(record) => Ok(record),
        _ => Err(corrupt("fleet project")),
    })
    .collect()
}
fn archive_reports(tx: &mut dyn SqlTx, board: &BoardId) -> Result<Vec<ProjectId>, CentralError> {
    Ok(crate::archive_receiver::reports(tx, board)?
        .into_iter()
        .map(|report| report.project)
        .collect())
}
fn name_owner(
    tx: &mut dyn SqlTx,
    scope: &FleetNameScope,
    name: &ResourceName,
) -> Result<Option<RecordRef>, CentralError> {
    let (sql, parameters, kind) = match scope {
        FleetNameScope::Workspaces {} => (
            "SELECT workspace_id AS id FROM fleet_workspaces WHERE name=?",
            vec![text(name.as_str())],
            RecordKind::Workspace,
        ),
        FleetNameScope::WorkspaceBoards(workspace) => (
            "SELECT board_id AS id FROM fleet_boards WHERE workspace_id=? AND name=?",
            vec![text(workspace.as_str()), text(name.as_str())],
            RecordKind::Board,
        ),
        FleetNameScope::BoardProjects(board) => (
            "SELECT project_id AS id FROM fleet_directory WHERE board_id=? AND title=?",
            vec![text(board.as_str()), text(name.as_str())],
            RecordKind::Project,
        ),
    };
    let rows = tx.query(query_statement(sql, parameters, ["id"]))?;
    rows.first()
        .map(|row| {
            Ok(RecordRef {
                kind,
                id: RecordId::try_from(row.text("id")?.to_owned())
                    .map_err(|_| corrupt("fleet name owner"))?,
            })
        })
        .transpose()
}
