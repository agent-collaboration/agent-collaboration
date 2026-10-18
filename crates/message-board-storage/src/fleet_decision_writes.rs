use crate::{
    FleetStorageError, fleet_record_codec::validate_fleet_record,
    project_record_io::record_kind_tag,
};
use fleet_admission::{FleetDecision, FleetWrite};
use message_board::{Record, RecordPosition};
use sqlx_turso::TursoConnection;

pub(crate) async fn apply_decision(
    connection: &mut TursoConnection,
    decision: &FleetDecision,
) -> Result<(), FleetStorageError> {
    for write in &decision.writes {
        match write {
            FleetWrite::Put(record) => put_record(connection, record).await?,
            FleetWrite::Ledger(ledger) => {
                put_record(connection, &Record::Request(ledger.as_ref().clone())).await?;
                let id = ledger.request_id.as_str();
                let hash = ledger.canonical_hash.as_str();
                let payload = serde_json::to_string(ledger)?;
                sqlx_turso::query!("INSERT INTO fleet_request_ledger(request_id, canonical_hash, payload) VALUES (?, ?, ?)", id, hash, payload).execute(&mut *connection).await?;
            }
            FleetWrite::RepositoryAttach {
                workspace,
                repository,
            } => {
                let workspace = workspace.as_str();
                let repository = serde_json::to_string(repository)?;
                sqlx_turso::query!(
                    "INSERT INTO fleet_repositories(workspace_id, reference) VALUES (?, ?)",
                    workspace,
                    repository
                )
                .execute(&mut *connection)
                .await?;
            }
            FleetWrite::RepositoryDetach {
                workspace,
                repository,
            } => {
                let workspace = workspace.as_str();
                let repository = serde_json::to_string(repository)?;
                sqlx_turso::query!(
                    "DELETE FROM fleet_repositories WHERE workspace_id = ? AND reference = ?",
                    workspace,
                    repository
                )
                .execute(&mut *connection)
                .await?;
            }
        }
    }
    append_feed(connection, &decision.feed).await
}

pub(crate) async fn append_feed(
    connection: &mut TursoConnection,
    entries: &[fleet_admission::FleetFeedEntry],
) -> Result<(), FleetStorageError> {
    let mut previous = crate::fleet_store::checkpoint(connection).await?;
    for entry in entries {
        if entry.position
            != previous
                .successor()
                .map_err(|_| FleetStorageError::InvalidRecord)?
            || entry.record.envelope().position != RecordPosition::Fleet(entry.position)
        {
            return Err(FleetStorageError::InvalidRecord);
        }
        let position =
            i64::try_from(entry.position.get()).map_err(|_| FleetStorageError::InvalidRecord)?;
        let payload = serde_json::to_string(entry)?;
        sqlx_turso::query!(
            "INSERT INTO fleet_feed(position, entry) VALUES (?, ?)",
            position,
            payload
        )
        .execute(&mut *connection)
        .await?;
        sqlx_turso::query!(
            "UPDATE fleet_checkpoint SET last_position = ? WHERE singleton = 1",
            position
        )
        .execute(&mut *connection)
        .await?;
        previous = entry.position;
    }
    Ok(())
}

pub(crate) async fn put_record(
    connection: &mut TursoConnection,
    record: &Record,
) -> Result<(), FleetStorageError> {
    validate_fleet_record(record)?;
    let reference = record.reference();
    let kind = record_kind_tag(reference.kind);
    let id = reference.id.as_str();
    let workspace = record.envelope().scope.workspace.as_str();
    let payload = serde_json::to_string(record)?;
    if matches!(record, Record::ProcessDefinition(_)) {
        sqlx_turso::query!(
            "INSERT INTO fleet_records(kind, id, workspace, payload) VALUES (?, ?, ?, ?)",
            kind,
            id,
            workspace,
            payload
        )
        .execute(&mut *connection)
        .await?;
    } else {
        sqlx_turso::query!("INSERT INTO fleet_records(kind, id, workspace, payload) VALUES (?, ?, ?, ?) ON CONFLICT(kind, id) DO UPDATE SET payload = excluded.payload", kind, id, workspace, payload).execute(&mut *connection).await?;
    }
    match record {
        Record::Workspace(row) => {
            let id = row.workspace_id.as_str();
            let name = row.name.as_str();
            sqlx_turso::query!("INSERT INTO fleet_workspaces(workspace_id, name) VALUES (?, ?) ON CONFLICT(workspace_id) DO UPDATE SET name = excluded.name", id, name).execute(connection).await?;
        }
        Record::Board(row) => {
            let id = row.board_id.as_str();
            let name = row.name.as_str();
            let workspace = row.workspace_id.as_str();
            sqlx_turso::query!("INSERT INTO fleet_boards(board_id, workspace_id, name) VALUES (?, ?, ?) ON CONFLICT(board_id) DO UPDATE SET name = excluded.name", id, workspace, name).execute(connection).await?;
        }
        Record::DirectoryEntry(row) => {
            let id = row.id.as_str();
            let board = row.board.as_str();
            let title = row.title.as_str();
            sqlx_turso::query!("INSERT INTO fleet_directory(project_id, board_id, title) VALUES (?, ?, ?) ON CONFLICT(project_id) DO UPDATE SET title = excluded.title", id, board, title).execute(connection).await?;
        }
        Record::ProcessDefinition(row) => {
            let name = row.reference.name.as_str();
            let version = row.reference.version.as_str();
            sqlx_turso::query!("INSERT INTO fleet_definitions(workspace_id, name, version, record_id) VALUES (?, ?, ?, ?)", workspace, name, version, id).execute(connection).await?;
        }
        Record::Adr(_) | Record::Request(_) => {}
        _ => return Err(FleetStorageError::InvalidRecord),
    }
    Ok(())
}
