//! Directory reads and write planning against the caller's central store.
use central_storage::{GuardedStatement, ReadOutcome, Statement, StoreFailure};
use fleet_contracts::{
    DirectoryWorkspace, MAXIMUM_DESCRIPTION_BYTES, WorkspaceDirectory, WorkspaceView,
};
use fleet_runtime_ports::CentralStore;
use fleet_runtime_ports::{
    guarded_statements::{exact, text},
    stored_values::string,
    workspace_decode_error::corrupt,
};
use workspace_model::{Description, ResourceName, Timestamp, WorkspaceId, WorkspaceState};

fn decode(rows: &ReadOutcome) -> Result<Option<DirectoryWorkspace>, StoreFailure> {
    if rows.rows.len() > 1 {
        return Err(corrupt("directory_workspaces.workspace_id"));
    }
    rows.rows
        .first()
        .map(|row| decode_row(rows, row))
        .transpose()
}

fn decode_row(
    rows: &ReadOutcome,
    row: &[central_storage::SqlValue],
) -> Result<DirectoryWorkspace, StoreFailure> {
    let workspace_id = string(rows, row, "workspace_id")?
        .parse()
        .map_err(|_| corrupt("directory_workspaces.workspace_id"))?;
    let stored_name = string(rows, row, "name")?;
    let name = ResourceName::try_from(stored_name.to_owned())
        .map_err(|_| corrupt("directory_workspaces.name"))?;
    if name.as_str() != stored_name {
        return Err(corrupt("directory_workspaces.name"));
    }
    let stored_description = string(rows, row, "description")?;
    if stored_description.len() > MAXIMUM_DESCRIPTION_BYTES {
        return Err(corrupt("directory_workspaces.description"));
    }
    let description = Description::try_from(stored_description.to_owned())
        .map_err(|_| corrupt("directory_workspaces.description"))?;
    let state = match string(rows, row, "state")? {
        "active" => WorkspaceState::Active,
        "archived" => WorkspaceState::Archived,
        _ => return Err(corrupt("directory_workspaces.state")),
    };
    for column in ["created_at", "updated_at"] {
        let _: Timestamp = string(rows, row, column)?
            .parse()
            .map_err(|_| corrupt(&format!("workspaces.{column}")))?;
    }
    Ok(DirectoryWorkspace {
        workspace_id,
        name,
        description,
        state,
    })
}
pub struct CentralWorkspaceDirectory<'store, Host: CentralStore> {
    host: &'store Host,
}

impl<'store, Host: CentralStore> CentralWorkspaceDirectory<'store, Host> {
    pub fn new(host: &'store Host) -> Self {
        Self { host }
    }

    /// Every workspace of the organization, archived ones included.
    pub async fn read_all(&self) -> Result<Vec<DirectoryWorkspace>, StoreFailure> {
        let rows = self.host.read(Statement {
            sql: "SELECT workspace_id,name,description,state,created_at,updated_at FROM directory_workspaces ORDER BY workspace_id",
            args: vec![],
        }).await?;
        rows.rows.iter().map(|row| decode_row(&rows, row)).collect()
    }
}

impl<Host: CentralStore> WorkspaceDirectory for CentralWorkspaceDirectory<'_, Host> {
    async fn read_by_id(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<Option<DirectoryWorkspace>, StoreFailure> {
        let rows = self.host.read(Statement {
            sql: "SELECT workspace_id,name,description,state,created_at,updated_at FROM directory_workspaces WHERE workspace_id=?",
            args: vec![text(workspace_id)],
        }).await?;
        let workspace = decode(&rows)?;
        if workspace
            .as_ref()
            .is_some_and(|workspace| workspace.workspace_id != *workspace_id)
        {
            return Err(corrupt("directory_workspaces.workspace_id"));
        }
        Ok(workspace)
    }

    async fn read_by_name(
        &self,
        name: &ResourceName,
    ) -> Result<Option<DirectoryWorkspace>, StoreFailure> {
        // Match workspaces_canonical_name exactly: SQLite folds ASCII only and trim()
        // removes surrounding ASCII spaces. No Rust/Unicode comparison substitutes.
        let rows = self.host.read(Statement {
            sql: "SELECT workspace_id,name,description,state,created_at,updated_at FROM directory_workspaces WHERE lower(trim(name))=lower(trim(?))",
            args: vec![text(name.as_str())],
        }).await?;
        decode(&rows)
    }

    fn plan_guarded_insert(
        &self,
        workspace_id: &WorkspaceId,
        name: &ResourceName,
        description: &Description,
        now: Timestamp,
    ) -> Result<GuardedStatement, StoreFailure> {
        Ok(exact(
            "INSERT INTO directory_workspaces(workspace_id,name,description,state,created_at,updated_at) VALUES (?,?,?,'active',?,?)",
            vec![
                text(workspace_id),
                text(name.as_str()),
                text(description.as_str()),
                text(now.to_rfc3339()),
                text(now.to_rfc3339()),
            ],
        ))
    }

    fn workspace_view(&self, row: DirectoryWorkspace) -> Result<WorkspaceView, StoreFailure> {
        Ok(WorkspaceView {
            workspace_id: row
                .workspace_id
                .to_string()
                .try_into()
                .map_err(|_| corrupt("directory_workspaces.workspace_id"))?,
            name: row
                .name
                .as_str()
                .to_owned()
                .try_into()
                .map_err(|_| corrupt("directory_workspaces.name"))?,
            description: row
                .description
                .as_str()
                .to_owned()
                .try_into()
                .map_err(|_| corrupt("directory_workspaces.description"))?,
            state: match row.state {
                WorkspaceState::Active => message_board::WorkspaceState::Active,
                WorkspaceState::Archived => message_board::WorkspaceState::Archived,
            },
        })
    }
}

#[cfg(test)]
#[path = "workspace_directory_rows_test.rs"]
mod tests;
