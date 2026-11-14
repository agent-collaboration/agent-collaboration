//! Shared modern directory rows; each caller retains its own decision and replay contract.
use crate::{
    ObjectHost,
    guarded_statements::{exact, text},
    workspace_decode_error::corrupt,
    workspace_row_codec::string,
};
use central_storage::{GuardedStatement, ReadOutcome, Statement, StoreFailure};
use workspace_model::{Description, ResourceName, Timestamp, WorkspaceId, WorkspaceState};

pub(crate) struct DirectoryWorkspace {
    pub workspace_id: WorkspaceId,
    pub name: ResourceName,
    pub description: Description,
    pub state: WorkspaceState,
}

fn decode(rows: &ReadOutcome) -> Result<Option<DirectoryWorkspace>, StoreFailure> {
    if rows.rows.len() > 1 {
        return Err(corrupt("workspaces.workspace_id"));
    }
    let Some(row) = rows.rows.first() else {
        return Ok(None);
    };
    let workspace_id = string(rows, row, "workspace_id")?
        .parse()
        .map_err(|_| corrupt("workspaces.workspace_id"))?;
    let stored_name = string(rows, row, "name")?;
    let name =
        ResourceName::try_from(stored_name.to_owned()).map_err(|_| corrupt("workspaces.name"))?;
    if name.as_str() != stored_name {
        return Err(corrupt("workspaces.name"));
    }
    let description = Description::try_from(string(rows, row, "description")?.to_owned())
        .map_err(|_| corrupt("workspaces.description"))?;
    let state = match string(rows, row, "state")? {
        "active" => WorkspaceState::Active,
        "archived" => WorkspaceState::Archived,
        _ => return Err(corrupt("workspaces.state")),
    };
    for column in ["created_at", "updated_at"] {
        let _: Timestamp = string(rows, row, column)?
            .parse()
            .map_err(|_| corrupt(&format!("workspaces.{column}")))?;
    }
    Ok(Some(DirectoryWorkspace {
        workspace_id,
        name,
        description,
        state,
    }))
}

pub(crate) async fn read_by_id(
    host: &impl ObjectHost,
    workspace_id: &WorkspaceId,
) -> Result<Option<DirectoryWorkspace>, StoreFailure> {
    let rows = host.read(Statement {
        sql: "SELECT workspace_id,name,description,state,created_at,updated_at FROM workspaces WHERE workspace_id=?",
        args: vec![text(workspace_id)],
    }).await?;
    let workspace = decode(&rows)?;
    if workspace
        .as_ref()
        .is_some_and(|workspace| workspace.workspace_id != *workspace_id)
    {
        return Err(corrupt("workspaces.workspace_id"));
    }
    Ok(workspace)
}

pub(crate) async fn read_by_name(
    host: &impl ObjectHost,
    name: &ResourceName,
) -> Result<Option<DirectoryWorkspace>, StoreFailure> {
    // Match workspaces_canonical_name exactly: SQLite folds ASCII only and trim()
    // removes surrounding ASCII spaces. No Rust/Unicode comparison substitutes.
    let rows = host.read(Statement {
        sql: "SELECT workspace_id,name,description,state,created_at,updated_at FROM workspaces WHERE lower(trim(name))=lower(trim(?))",
        args: vec![text(name.as_str())],
    }).await?;
    decode(&rows)
}

pub(crate) fn insert(
    workspace_id: &WorkspaceId,
    name: &ResourceName,
    description: &Description,
    now: Timestamp,
) -> Result<GuardedStatement, StoreFailure> {
    Ok(exact(
        "INSERT INTO workspaces(workspace_id,name,description,state,created_at,updated_at) VALUES (?,?,?,'active',?,?)",
        vec![
            text(workspace_id),
            text(name.as_str()),
            text(description.as_str()),
            text(now.to_rfc3339()),
            text(now.to_rfc3339()),
        ],
    ))
}
