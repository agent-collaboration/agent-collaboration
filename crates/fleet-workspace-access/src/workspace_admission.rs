//! Live request admission. Callers hold the central turn through the admitted effect.
use crate::WorkspaceAccess;
use central_storage::{SqlValue, Statement, StoreFailure};
use fleet_contracts::{LiveMachineConnection, MachineConnectionAttachment};
use fleet_runtime_ports::stored_values::{column, json, string};
use fleet_runtime_ports::workspace_decode_error::corrupt;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use workspace_ids::MachineId;
use workspace_model::{Caller, RefusalReason, WorkspaceId};

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceAccess<'_, Host, Stores> {
    pub async fn admit_workspace(
        &self,
        live: &impl LiveMachineConnection,
        caller: &Caller,
        attachment: &MachineConnectionAttachment,
        workspace: Option<&WorkspaceId>,
    ) -> Result<Result<(), RefusalReason>, StoreFailure> {
        if attachment.organization_id() != self.execution.organization()
            || caller.machine_id() != attachment.machine_id()
        {
            return Ok(Err(RefusalReason::MachineRevoked));
        }
        if live.restore_held(attachment).await?.is_err() {
            return Ok(Err(RefusalReason::MachineRevoked));
        }
        if let Some(workspace) = workspace {
            let rows = self.execution.statement_host().read(Statement {
                sql: "SELECT m.allowed_workspaces,w.state AS workspace_state FROM login_machines m LEFT JOIN directory_workspaces w ON w.workspace_id=? WHERE m.machine_id=?",
                args: vec![SqlValue::Text(workspace.to_string()), SqlValue::Text(attachment.machine_id().as_str().to_owned())],
            }).await?;
            let Some(row) = rows.rows.first() else {
                return Ok(Err(RefusalReason::MachineRevoked));
            };
            if rows.rows.len() != 1 {
                return Err(corrupt("login_machines.machine_id"));
            }
            let allowed: Vec<WorkspaceId> = json(
                string(&rows, row, "allowed_workspaces")?,
                "login_machines.allowed_workspaces",
            )?;
            if !allowed.contains(workspace) {
                return Ok(Err(RefusalReason::WorkspaceNotAllowed));
            }
            match column(&rows, row, "workspace_state")? {
                SqlValue::Text(value) if value == "active" => {}
                SqlValue::Text(value) if value == "archived" => {
                    return Ok(Err(RefusalReason::WorkspaceNotAllowed));
                }
                SqlValue::Null => return Ok(Err(RefusalReason::WorkspaceNotAllowed)),
                _ => return Err(corrupt("directory_workspaces.state")),
            }
        }
        Ok(Ok(()))
    }
    /// The workspaces the admitted caller's machine may use. Call after admission.
    pub async fn allowed_workspaces(
        &self,
        machine_id: &MachineId,
    ) -> Result<Vec<WorkspaceId>, StoreFailure> {
        let rows = self
            .execution
            .statement_host()
            .read(Statement {
                sql: "SELECT allowed_workspaces FROM login_machines WHERE machine_id=?",
                args: vec![SqlValue::Text(machine_id.as_str().to_owned())],
            })
            .await?;
        let row = rows
            .rows
            .first()
            .ok_or_else(|| corrupt("login_machines.machine_id"))?;
        json(
            string(&rows, row, "allowed_workspaces")?,
            "allowed_workspaces",
        )
    }
}
