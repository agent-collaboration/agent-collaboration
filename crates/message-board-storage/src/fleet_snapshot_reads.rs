use crate::{FleetStorageError, fleet_record_codec::read_fleet_record};
use fleet_admission::{
    FleetNameOwner, FleetNameScope, FleetReadKey, FleetReads, FleetRequest, FleetSnapshot,
    reads_for,
};
use message_board::*;
use sqlx_turso::TursoConnection;

pub(crate) async fn read_snapshot(
    connection: &mut TursoConnection,
    request: &FleetRequest,
) -> Result<FleetSnapshot, FleetStorageError> {
    read_keys(connection, reads_for(request)).await
}

pub(crate) async fn read_keys(
    connection: &mut TursoConnection,
    reads: FleetReads,
) -> Result<FleetSnapshot, FleetStorageError> {
    let mut snapshot = FleetSnapshot::empty();
    for key in reads.keys {
        match key {
            // The Router stand-in does not import; ordinary reads_for never
            // emits this central-owned metadata key.
            FleetReadKey::ImportDigest { .. } => return Err(FleetStorageError::InvalidRecord),
            FleetReadKey::FeedHighWater {} => {
                snapshot.high_water = crate::fleet_store::checkpoint(connection).await?
            }
            FleetReadKey::Ledger { request: id } => {
                let id = id.as_str();
                if let Some(row) = sqlx_turso::query!(r#"SELECT canonical_hash AS "canonical_hash!: String", payload AS "payload!: String" FROM fleet_request_ledger WHERE request_id = ?"#, id).fetch_optional(&mut *connection).await? {
                    let ledger: RequestLedgerEntry = serde_json::from_str(&row.payload).map_err(|_| FleetStorageError::InvalidRecord)?;
                    if ledger.request_id.as_str() != id || ledger.canonical_hash.as_str() != row.canonical_hash { return Err(FleetStorageError::InvalidRecord); }
                    crate::fleet_record_codec::validate_fleet_record(&Record::Request(ledger.clone()))?;
                    snapshot.ledger = Some(ledger);
                }
            }
            FleetReadKey::Record { record } => {
                if let Some(row) = read_fleet_record(connection, &record).await? {
                    snapshot.records.push(row);
                }
            }
            FleetReadKey::Name { scope, name } => {
                if let Some(record) = name_owner(connection, &scope, &name).await? {
                    snapshot.names.push(FleetNameOwner {
                        scope,
                        name,
                        record,
                    });
                }
            }
            FleetReadKey::Definition {
                workspace,
                name,
                version,
            } => {
                let workspace = workspace.as_str();
                let name = name.as_str();
                let version = version.as_str();
                if let Some(id) = sqlx_turso::query_scalar!(r#"SELECT record_id AS "record_id!: String" FROM fleet_definitions WHERE workspace_id = ? AND name = ? AND version = ?"#, workspace, name, version).fetch_optional(&mut *connection).await? {
                    let reference = RecordRef { kind: RecordKind::ProcessDefinition, id: RecordId::try_from(id).map_err(|_| FleetStorageError::InvalidRecord)? };
                    match read_fleet_record(connection, &reference).await? { Some(Record::ProcessDefinition(row)) => snapshot.definitions.push(row), _ => return Err(FleetStorageError::InvalidRecord) }
                }
            }
            FleetReadKey::WorkspaceBoards { workspace } => {
                snapshot.workspace_boards = workspace_boards(connection, &workspace).await?;
            }
            FleetReadKey::WorkspaceArchiveInputs { workspace } => {
                snapshot.workspace_boards = workspace_boards(connection, &workspace).await?;
                for board in &snapshot.workspace_boards {
                    let id = board.board_id.as_str();
                    let projects=sqlx_turso::query_scalar!(r#"SELECT project_id AS "project_id!: String" FROM fleet_directory WHERE board_id = ? ORDER BY project_id"#,id).fetch_all(&mut *connection).await?;
                    for project in projects {
                        let reference = RecordRef {
                            kind: RecordKind::Project,
                            id: RecordId::try_from(project)
                                .map_err(|_| FleetStorageError::InvalidRecord)?,
                        };
                        let Some(Record::DirectoryEntry(project)) =
                            read_fleet_record(connection, &reference).await?
                        else {
                            return Err(FleetStorageError::InvalidRecord);
                        };
                        snapshot.board_projects.push(project);
                    }
                    for report in
                        crate::fleet_archive_reports::reports(connection, &board.board_id).await?
                    {
                        snapshot.archive_reports.push(report.project);
                    }
                }
            }
            FleetReadKey::BoardProjects { board } => {
                let board = board.as_str();
                let ids = sqlx_turso::query_scalar!(r#"SELECT project_id AS "project_id!: String" FROM fleet_directory WHERE board_id = ? ORDER BY project_id"#, board).fetch_all(&mut *connection).await?;
                for id in ids {
                    let reference = RecordRef {
                        kind: RecordKind::Project,
                        id: RecordId::try_from(id).map_err(|_| FleetStorageError::InvalidRecord)?,
                    };
                    match read_fleet_record(connection, &reference).await? {
                        Some(Record::DirectoryEntry(row)) => snapshot.board_projects.push(row),
                        _ => return Err(FleetStorageError::InvalidRecord),
                    }
                }
            }
            FleetReadKey::ArchiveReports { board } => {
                snapshot.archive_reports =
                    crate::fleet_archive_reports::reports(connection, &board)
                        .await?
                        .into_iter()
                        .map(|report| report.project)
                        .collect();
            }
            FleetReadKey::Repositories { workspace } => {
                let workspace = workspace.as_str();
                let rows = sqlx_turso::query_scalar!(r#"SELECT reference AS "reference!: String" FROM fleet_repositories WHERE workspace_id = ? ORDER BY reference"#, workspace).fetch_all(&mut *connection).await?;
                snapshot.repositories = rows
                    .into_iter()
                    .map(|row| {
                        serde_json::from_str(&row).map_err(|_| FleetStorageError::InvalidRecord)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
            }
        }
    }
    Ok(snapshot)
}

async fn workspace_boards(
    connection: &mut TursoConnection,
    workspace: &WorkspaceId,
) -> Result<Vec<Board>, FleetStorageError> {
    let workspace = workspace.as_str();
    let ids=sqlx_turso::query_scalar!(r#"SELECT board_id AS "board_id!: String" FROM fleet_boards WHERE workspace_id = ? ORDER BY board_id"#,workspace).fetch_all(&mut *connection).await?;
    let mut boards = Vec::with_capacity(ids.len());
    for id in ids {
        let reference = RecordRef {
            kind: RecordKind::Board,
            id: RecordId::try_from(id).map_err(|_| FleetStorageError::InvalidRecord)?,
        };
        let Some(Record::Board(board)) = read_fleet_record(connection, &reference).await? else {
            return Err(FleetStorageError::InvalidRecord);
        };
        boards.push(board);
    }
    Ok(boards)
}

async fn name_owner(
    connection: &mut TursoConnection,
    scope: &FleetNameScope,
    name: &ResourceName,
) -> Result<Option<RecordRef>, FleetStorageError> {
    let name = name.as_str();
    let (kind, id) = match scope {
        FleetNameScope::Workspaces {} => (RecordKind::Workspace, sqlx_turso::query_scalar!(r#"SELECT workspace_id AS "workspace_id!: String" FROM fleet_workspaces WHERE name = ?"#, name).fetch_optional(connection).await?),
        FleetNameScope::WorkspaceBoards(workspace) => {
            let workspace = workspace.as_str();
            (RecordKind::Board, sqlx_turso::query_scalar!(r#"SELECT board_id AS "board_id!: String" FROM fleet_boards WHERE workspace_id = ? AND name = ?"#, workspace, name).fetch_optional(connection).await?)
        }
        FleetNameScope::BoardProjects(board) => {
            let board = board.as_str();
            (RecordKind::Project, sqlx_turso::query_scalar!(r#"SELECT project_id AS "project_id!: String" FROM fleet_directory WHERE board_id = ? AND title = ?"#, board, name).fetch_optional(connection).await?)
        }
    };
    id.map(|id| {
        Ok(RecordRef {
            kind,
            id: RecordId::try_from(id).map_err(|_| FleetStorageError::InvalidRecord)?,
        })
    })
    .transpose()
}
