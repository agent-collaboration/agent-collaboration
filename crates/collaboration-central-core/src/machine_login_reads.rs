//! Read only the machine-login facts named by an organization-serialized query.
use crate::{
    ObjectHost,
    guarded_statements::text,
    machine_login_row_values::{identifier, nonnegative_integer},
    machine_login_rows::{enrollment, machine, only_row},
    organization_authority::machine_login::{EnrollmentFacts, EnrollmentQuery},
    workspace_decode_error::corrupt,
    workspace_row_codec::string,
};
use central_storage::{Statement, StoreFailure};
use fleet_identity_protocol::OrganizationId;

const BY_REQUEST: &str = "SELECT a.approval_id,a.request_id,a.machine_id,a.label,a.allowed_workspaces,a.state,a.end_reason,a.approved_by_person_id,a.permission_checked,a.base_generation,a.code_expires_at,a.machine_client_id,a.last_grant_at,a.ended_at,a.created_at,a.updated_at,m.installation_id FROM machine_approvals a LEFT JOIN machines m ON m.machine_id=a.machine_id WHERE a.request_id=?";
const BY_ENROLLMENT: &str = "SELECT a.approval_id,a.request_id,a.machine_id,a.label,a.allowed_workspaces,a.state,a.end_reason,a.approved_by_person_id,a.permission_checked,a.base_generation,a.code_expires_at,a.machine_client_id,a.last_grant_at,a.ended_at,a.created_at,a.updated_at,m.installation_id FROM machine_approvals a LEFT JOIN machines m ON m.machine_id=a.machine_id WHERE a.approval_id=?";
const ACTIVE: &str = "SELECT a.approval_id,a.request_id,a.machine_id,a.label,a.allowed_workspaces,a.state,a.end_reason,a.approved_by_person_id,a.permission_checked,a.base_generation,a.code_expires_at,a.machine_client_id,a.last_grant_at,a.ended_at,a.created_at,a.updated_at,m.installation_id FROM machine_approvals a LEFT JOIN machines m ON m.machine_id=a.machine_id WHERE a.machine_id=? AND a.state='enrolled'";

pub(crate) async fn facts(
    host: &impl ObjectHost,
    organization_id: OrganizationId,
    query: &EnrollmentQuery,
) -> Result<EnrollmentFacts, StoreFailure> {
    let prior_approval = if let Some(request) = &query.request {
        let rows = host
            .read(Statement {
                sql: BY_REQUEST,
                args: vec![text(request.as_str())],
            })
            .await?;
        only_row(&rows, enrollment, "machine_approvals.request_id")?
    } else {
        None
    };
    if prior_approval.is_some() {
        return Ok(EnrollmentFacts {
            organization_id,
            organization_has_enrollment: true,
            prior_approval,
            enrollment: None,
            machine: None,
            active: None,
            missing_workspaces: vec![],
        });
    }
    let history = host
        .read(Statement {
            sql: "SELECT EXISTS(SELECT 1 FROM machine_approvals) AS has_enrollment",
            args: vec![],
        })
        .await?;
    let organization_has_enrollment = only_row(
        &history,
        |rows, row| match nonnegative_integer(rows, row, "has_enrollment")?.get() {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(corrupt("machine_approvals.has_enrollment")),
        },
        "machine_approvals.has_enrollment",
    )?
    .ok_or_else(|| corrupt("machine_approvals.has_enrollment"))?;
    let enrollment = if let Some(id) = &query.enrollment {
        let rows = host
            .read(Statement {
                sql: BY_ENROLLMENT,
                args: vec![text(id.as_str())],
            })
            .await?;
        only_row(&rows, enrollment, "machine_approvals.approval_id")?
    } else {
        None
    };
    let machine = if let Some(installation) = &query.installation {
        let rows = host.read(Statement { sql: "SELECT machine_id,installation_id,label,state,allowed_workspaces,enrollment_generation,created_at,updated_at FROM machines WHERE installation_id=?", args: vec![text(installation.as_str())] }).await?;
        only_row(&rows, machine, "machines.installation_id")?
    } else if let Some(found) = &enrollment {
        let rows = host.read(Statement { sql: "SELECT machine_id,installation_id,label,state,allowed_workspaces,enrollment_generation,created_at,updated_at FROM machines WHERE machine_id=?", args: vec![text(found.machine_id)] }).await?;
        Some(
            only_row(&rows, machine, "machines.machine_id")?
                .ok_or_else(|| corrupt("machine_approvals.machine_id"))?,
        )
    } else {
        None
    };
    let active = if let Some(found) = &machine {
        let rows = host
            .read(Statement {
                sql: ACTIVE,
                args: vec![text(found.id)],
            })
            .await?;
        only_row(
            &rows,
            crate::machine_login_rows::enrollment,
            "machine_approvals.enrolled",
        )?
    } else {
        None
    };
    let mut missing_workspaces = Vec::new();
    for workspace in &query.workspaces {
        let rows = host
            .read(Statement {
                sql: "SELECT workspace_id FROM workspaces WHERE workspace_id=?",
                args: vec![text(workspace.as_str())],
            })
            .await?;
        let found: Option<message_board::WorkspaceId> = only_row(
            &rows,
            |rows, row| {
                identifier(
                    string(rows, row, "workspace_id")?,
                    "workspaces.workspace_id",
                )
            },
            "workspaces.workspace_id",
        )?;
        if found.is_none() {
            missing_workspaces.push(workspace.clone());
        }
    }
    Ok(EnrollmentFacts {
        organization_id,
        organization_has_enrollment,
        prior_approval,
        enrollment,
        machine,
        active,
        missing_workspaces,
    })
}

#[cfg(test)]
#[path = "machine_login_reads_test.rs"]
mod tests;
