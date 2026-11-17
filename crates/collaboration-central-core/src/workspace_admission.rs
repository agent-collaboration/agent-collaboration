use crate::workspace_decode_error::corrupt;
use crate::workspace_row_codec::{json, string};
use crate::{ObjectHost, WorkspaceCaller, WorkspaceRuntime, WorkspaceStoreCalls};
use central_storage::{SqlValue, Statement, StoreFailure};
use workspace_model::{MachineId, RefusalReason, WorkspaceId};

impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn admit_workspace(
        &self,
        caller: &WorkspaceCaller,
        workspace: Option<&WorkspaceId>,
    ) -> Result<Result<(), RefusalReason>, StoreFailure> {
        if caller.organization_id != self.organization {
            return Ok(Err(RefusalReason::MachineRevoked));
        }
        let rows=self.host.read(Statement {sql:"SELECT m.machine_id,m.state,m.allowed_workspaces,c.expires_at,w.state AS workspace_state FROM machines m JOIN machine_credentials c ON c.machine_id=m.machine_id LEFT JOIN workspaces w ON w.workspace_id=? WHERE m.machine_id=? AND c.token_hash=?",args:vec![workspace.map_or(SqlValue::Null,|id|SqlValue::Text(id.to_string())),SqlValue::Text(caller.caller.machine_id().to_string()),SqlValue::Text(caller.credential_hash.clone())]}).await?;
        let Some(row) = rows.rows.first() else {
            return Ok(Err(RefusalReason::MachineRevoked));
        };
        let _: MachineId = string(&rows, row, "machine_id")?
            .parse()
            .map_err(|_| corrupt("machine_id"))?;
        match string(&rows, row, "state")? {
            "enrolled" => {}
            "revoked" => return Ok(Err(RefusalReason::MachineRevoked)),
            _ => return Err(corrupt("machines.state")),
        }
        let expires: workspace_model::Timestamp = string(&rows, row, "expires_at")?
            .parse()
            .map_err(|_| corrupt("expires_at"))?;
        if expires <= self.now()? {
            return Ok(Err(RefusalReason::MachineRevoked));
        }
        let allowed: Vec<WorkspaceId> = json(
            string(&rows, row, "allowed_workspaces")?,
            "allowed_workspaces",
        )?;
        if let Some(workspace) = workspace {
            if !allowed.contains(workspace) {
                return Ok(Err(RefusalReason::WorkspaceNotAllowed));
            }
            match crate::workspace_row_codec::column(&rows, row, "workspace_state")? {
                SqlValue::Text(value) if value == "active" => {}
                SqlValue::Text(value) if value == "archived" => {
                    return Ok(Err(RefusalReason::WorkspaceNotAllowed));
                }
                SqlValue::Null => return Ok(Err(RefusalReason::WorkspaceNotAllowed)),
                _ => return Err(corrupt("workspaces.state")),
            }
        }
        Ok(Ok(()))
    }
    /// The workspaces the admitted caller's machine may use. Call after admission.
    pub(crate) async fn allowed_workspaces(
        &self,
        caller: &WorkspaceCaller,
    ) -> Result<Vec<WorkspaceId>, StoreFailure> {
        let rows = self
            .host
            .read(Statement {
                sql: "SELECT allowed_workspaces FROM machines WHERE machine_id=?",
                args: vec![SqlValue::Text(caller.caller.machine_id().to_string())],
            })
            .await?;
        let row = rows
            .rows
            .first()
            .ok_or_else(|| corrupt("machines.machine_id"))?;
        json(
            string(&rows, row, "allowed_workspaces")?,
            "allowed_workspaces",
        )
    }
    /// The machine a Router installation enrolled as. The installation id is
    /// any canonical RFC UUID, so live installations keep their v4 ids; the
    /// machine id is central's own UUIDv7.
    pub async fn machine_for_installation(
        &self,
        installation: &workspace_model::InstallationId,
    ) -> Result<Option<MachineId>, StoreFailure> {
        let rows = self
            .host
            .read(Statement {
                sql: "SELECT machine_id FROM machines WHERE installation_id=?",
                args: vec![SqlValue::Text(installation.as_str().to_owned())],
            })
            .await?;
        if rows.rows.len() > 1 {
            return Err(corrupt("machines.installation_id"));
        }
        rows.rows
            .first()
            .map(|row| {
                string(&rows, row, "machine_id")?
                    .parse()
                    .map_err(|_| corrupt("machines.machine_id"))
            })
            .transpose()
    }
}
