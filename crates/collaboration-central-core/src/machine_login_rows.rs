//! Decode owning machine/login rows into the pure policy's checked facts.
use crate::{
    identity_contract::{AllowedWorkspaces, ClientId, EnrollmentRevocationReason},
    machine_login_row_values as values,
    organization_authority::machine_login::{
        EnrollmentGeneration, MachineRow, MachineState, StoredEnrollment, StoredEnrollmentState,
    },
    workspace_decode_error::corrupt,
    workspace_row_codec::{json, string},
};
use central_storage::{ReadOutcome, SqlValue, StoreFailure};

fn label(value: &str, field: &str) -> Result<message_board::MachineLabel, StoreFailure> {
    let parsed =
        message_board::MachineLabel::try_from(value.to_owned()).map_err(|_| corrupt(field))?;
    if parsed.as_str() != value {
        return Err(corrupt(field));
    }
    Ok(parsed)
}

pub(crate) fn machine(rows: &ReadOutcome, row: &[SqlValue]) -> Result<MachineRow, StoreFailure> {
    values::row_millis(rows, row, "created_at")
        .map_err(|_| corrupt("login_machines.created_at"))?;
    values::row_millis(rows, row, "updated_at")
        .map_err(|_| corrupt("login_machines.updated_at"))?;
    let generation = values::nonnegative_integer(rows, row, "enrollment_generation")
        .map_err(|_| corrupt("login_machines.enrollment_generation"))?;
    Ok(MachineRow {
        machine_id: string(rows, row, "machine_id")?
            .parse()
            .map_err(|_| corrupt("login_machines.machine_id"))?,
        label: label(string(rows, row, "label")?, "login_machines.label")?,
        allowed_workspaces: json(
            string(rows, row, "allowed_workspaces")?,
            "login_machines.allowed_workspaces",
        )?,
        generation: EnrollmentGeneration::try_from(
            u64::try_from(generation.get())
                .map_err(|_| corrupt("login_machines.enrollment_generation"))?,
        )?,
        state: match string(rows, row, "state")? {
            "enrolled" => MachineState::Enrolled,
            "revoked" => MachineState::Revoked,
            _ => return Err(corrupt("login_machines.state")),
        },
    })
}

pub(crate) fn enrollment(
    rows: &ReadOutcome,
    row: &[SqlValue],
) -> Result<StoredEnrollment, StoreFailure> {
    let approved_at_ms = values::row_millis(rows, row, "created_at")
        .map_err(|_| corrupt("login_machine_approvals.created_at"))?;
    values::row_millis(rows, row, "updated_at")
        .map_err(|_| corrupt("login_machine_approvals.updated_at"))?;
    let base = values::nonnegative_integer(rows, row, "base_generation")
        .map_err(|_| corrupt("login_machine_approvals.base_generation"))?;
    let client: Option<ClientId> = values::optional_text(rows, row, "machine_client_id")?
        .map(|value| values::identifier(value, "login_machine_approvals.machine_client_id"))
        .transpose()?;
    let last_grant = values::optional_text(rows, row, "last_grant_at")?
        .map(|value| values::seconds(value, "login_machine_approvals.last_grant_at"))
        .transpose()?;
    let ended_at_ms = values::optional_text(rows, row, "ended_at")?
        .map(|value| values::millis(value, "login_machine_approvals.ended_at"))
        .transpose()?;
    let reason = values::optional_text(rows, row, "end_reason")?;
    let state = match (
        string(rows, row, "state")?,
        reason,
        client,
        last_grant,
        ended_at_ms,
    ) {
        ("approved", None, None, None, None) => StoredEnrollmentState::Approved,
        ("enrolled", None, Some(client_id), Some(last_grant_at), None) => {
            StoredEnrollmentState::Enrolled {
                client_id,
                last_grant_at,
            }
        }
        ("ended", Some("lapsed"), None, None, Some(_)) => StoredEnrollmentState::Lapsed,
        ("ended", Some("owner"), Some(client_id), None, Some(_)) => {
            StoredEnrollmentState::Revoked {
                client_id,
                reason: EnrollmentRevocationReason::Owner {},
            }
        }
        ("ended", Some("superseded"), Some(client_id), None, Some(_)) => {
            StoredEnrollmentState::Revoked {
                client_id,
                reason: EnrollmentRevocationReason::Superseded {},
            }
        }
        ("ended", Some("idle"), Some(client_id), None, Some(_)) => {
            StoredEnrollmentState::Expired { client_id }
        }
        _ => return Err(corrupt("login_machine_approvals.state")),
    };
    if string(rows, row, "permission_checked")? != "machine:enroll" {
        return Err(corrupt("login_machine_approvals.permission_checked"));
    }
    let allowed_workspaces: AllowedWorkspaces = json(
        string(rows, row, "allowed_workspaces")?,
        "login_machine_approvals.allowed_workspaces",
    )?;
    Ok(StoredEnrollment {
        id: values::identifier(
            string(rows, row, "approval_id")?,
            "login_machine_approvals.approval_id",
        )?,
        machine_id: string(rows, row, "machine_id")?
            .parse()
            .map_err(|_| corrupt("login_machine_approvals.machine_id"))?,
        request_id: values::identifier(
            string(rows, row, "request_id")?,
            "login_machine_approvals.request_id",
        )?,
        approved_by: values::identifier(
            string(rows, row, "approved_by_person_id")?,
            "login_machine_approvals.approved_by_person_id",
        )?,
        label: label(string(rows, row, "label")?, "login_machine_approvals.label")?,
        allowed_workspaces,
        base_generation: EnrollmentGeneration::try_from(
            u64::try_from(base.get())
                .map_err(|_| corrupt("login_machine_approvals.base_generation"))?,
        )?,
        approved_at_ms,
        code_expires_at: values::seconds(
            string(rows, row, "code_expires_at")?,
            "login_machine_approvals.code_expires_at",
        )?,
        state,
        ended_at_ms,
    })
}

pub(crate) fn only_row<T>(
    rows: &ReadOutcome,
    decode: impl FnOnce(&ReadOutcome, &[SqlValue]) -> Result<T, StoreFailure>,
    field: &str,
) -> Result<Option<T>, StoreFailure> {
    match rows.rows.as_slice() {
        [] => Ok(None),
        [row] => decode(rows, row).map(Some),
        _ => Err(corrupt(field)),
    }
}

#[cfg(test)]
#[path = "machine_login_rows_test.rs"]
mod tests;
