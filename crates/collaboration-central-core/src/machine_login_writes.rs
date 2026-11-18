//! Static guarded statements for a pure decision, in its declared atomic order.
use crate::{
    guarded_statements::{exact, text},
    identity_contract::EnrollmentRevocationReason,
    machine_login_row_values as values,
    organization_authority::machine_login::{
        EnrollmentChange, EnrollmentFacts, IDLE_EXPIRY_SECONDS, MachineRow, MachineState,
        StoredEnrollment, StoredEnrollmentState,
    },
};
use central_storage::{GuardedStatement, SafeInteger, SqlValue, StoreFailure};
use fleet_runtime::workspace_decode_error::corrupt;
use serde::Serialize;
use std::collections::BTreeMap;

pub(crate) fn statements(
    facts: &EnrollmentFacts,
    changes: &[EnrollmentChange],
    applying_ms: i64,
) -> Result<Vec<GuardedStatement>, StoreFailure> {
    let at = values::timestamp(applying_ms, "machine_login.applying_time")?;
    let mut machine = facts.machine.clone();
    let mut enrollments = BTreeMap::new();
    for enrollment in [&facts.enrollment, &facts.active, &facts.prior_approval]
        .into_iter()
        .flatten()
    {
        enrollments.insert(enrollment.id.as_str().to_owned(), enrollment.clone());
    }
    let mut statements = Vec::new();
    if !changes.is_empty()
        && let Some(before) = &machine
    {
        statements.push(exact("UPDATE machines SET machine_id=machine_id WHERE machine_id=? AND installation_id=? AND enrollment_generation=? AND state=? AND label=? AND json(allowed_workspaces)=json(?)", vec![text(before.id),text(before.installation.as_str()),generation(before.generation.get())?,text(machine_state(before.state)),text(before.label.as_str()),encode(&before.allowed_workspaces,"machines.allowed_workspaces")?]));
    }
    for change in changes {
        match change {
            EnrollmentChange::PutMachine(next) => {
                statements.push(put_machine(machine.as_ref(), next, &at)?);
                machine = Some(next.clone());
            }
            EnrollmentChange::InsertEnrollment(next) => {
                let owner = machine
                    .as_ref()
                    .ok_or_else(|| corrupt("machine_approvals.machine_id"))?;
                if owner.id != next.machine_id
                    || owner.installation != next.installation
                    || owner.generation != next.base_generation
                {
                    return Err(corrupt("machine_approvals.machine_id"));
                }
                statements.push(insert_enrollment(next, &at)?);
                enrollments.insert(next.id.as_str().to_owned(), next.clone());
            }
            EnrollmentChange::ReplaceEnrollment(next) => {
                let before = enrollments
                    .get(next.id.as_str())
                    .ok_or_else(|| corrupt("machine_approvals.approval_id"))?;
                if let Some(guard) = time_guard(before, next, applying_ms)? {
                    statements.push(guard);
                }
                statements.push(replace_enrollment(before, next, &at)?);
                enrollments.insert(next.id.as_str().to_owned(), next.clone());
            }
            EnrollmentChange::Record(record) => {
                if record.created_at_millis != applying_ms {
                    return Err(corrupt("organization_change_records.created_at"));
                }
                statements.push(crate::organization_change_rows::insert(record)?);
            }
        }
    }
    Ok(statements)
}

fn integer(value: i64, field: &str) -> Result<SqlValue, StoreFailure> {
    SafeInteger::new(value)
        .map(SqlValue::Integer)
        .map_err(|_| corrupt(field))
}
fn generation(value: u64) -> Result<SqlValue, StoreFailure> {
    integer(
        i64::try_from(value).map_err(|_| corrupt("machines.enrollment_generation"))?,
        "machines.enrollment_generation",
    )
}
fn encode(value: &impl Serialize, field: &str) -> Result<SqlValue, StoreFailure> {
    serde_json::to_string(value)
        .map(text)
        .map_err(|_| corrupt(field))
}
fn machine_state(state: MachineState) -> &'static str {
    match state {
        MachineState::Enrolled => "enrolled",
        MachineState::Revoked => "revoked",
    }
}
fn put_machine(
    before: Option<&MachineRow>,
    next: &MachineRow,
    at: &str,
) -> Result<GuardedStatement, StoreFailure> {
    values::private_id(&next.id.to_string(), "machines.machine_id")?;
    values::private_id(next.installation.as_str(), "machines.installation_id")?;
    let allowed = encode(&next.allowed_workspaces, "machines.allowed_workspaces")?;
    match before {
        None => {
            if next.generation.get() != 0 || next.state != MachineState::Revoked {
                return Err(corrupt("machines.enrollment_generation"));
            }
            Ok(exact(
                "INSERT INTO machines(machine_id,installation_id,label,state,allowed_workspaces,enrollment_generation,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?)",
                vec![
                    text(next.id),
                    text(next.installation.as_str()),
                    text(next.label.as_str()),
                    text(machine_state(next.state)),
                    allowed,
                    generation(next.generation.get())?,
                    text(at),
                    text(at),
                ],
            ))
        }
        Some(before) => {
            if before.id != next.id || before.installation != next.installation {
                return Err(corrupt("machines.machine_id"));
            }
            if next.generation != before.generation
                && next.generation != before.generation.next()?
            {
                return Err(corrupt("machines.enrollment_generation"));
            }
            Ok(exact(
                "UPDATE machines SET label=?,state=?,allowed_workspaces=?,enrollment_generation=?,updated_at=? WHERE machine_id=? AND installation_id=? AND enrollment_generation=? AND state=? AND label=? AND json(allowed_workspaces)=json(?)",
                vec![
                    text(next.label.as_str()),
                    text(machine_state(next.state)),
                    allowed,
                    generation(next.generation.get())?,
                    text(at),
                    text(before.id),
                    text(before.installation.as_str()),
                    generation(before.generation.get())?,
                    text(machine_state(before.state)),
                    text(before.label.as_str()),
                    encode(&before.allowed_workspaces, "machines.allowed_workspaces")?,
                ],
            ))
        }
    }
}

struct EncodedState {
    state: &'static str,
    reason: Option<&'static str>,
    client: Option<String>,
    last_grant: Option<String>,
    ended_at: Option<String>,
}
fn state(enrollment: &StoredEnrollment) -> Result<EncodedState, StoreFailure> {
    let ended_at = enrollment
        .ended_at_ms
        .map(|at| values::timestamp(at, "machine_approvals.ended_at"))
        .transpose()?;
    let (state, reason, client, last_grant) = match &enrollment.state {
        StoredEnrollmentState::Approved => ("approved", None, None, None),
        StoredEnrollmentState::Lapsed => ("ended", Some("lapsed"), None, None),
        StoredEnrollmentState::Enrolled {
            client_id,
            last_grant_at,
        } => (
            "enrolled",
            None,
            Some(client_id.as_str().to_owned()),
            Some(values::seconds_timestamp(
                *last_grant_at,
                "machine_approvals.last_grant_at",
            )?),
        ),
        StoredEnrollmentState::Revoked { client_id, reason } => (
            "ended",
            Some(match reason {
                EnrollmentRevocationReason::Owner {} => "owner",
                EnrollmentRevocationReason::Superseded {} => "superseded",
            }),
            Some(client_id.as_str().to_owned()),
            None,
        ),
        StoredEnrollmentState::Expired { client_id } => (
            "ended",
            Some("idle"),
            Some(client_id.as_str().to_owned()),
            None,
        ),
    };
    if (state == "ended") != ended_at.is_some() {
        return Err(corrupt("machine_approvals.ended_at"));
    }
    if let Some(client) = &client {
        values::private_id(client, "machine_approvals.machine_client_id")?;
    }
    Ok(EncodedState {
        state,
        reason,
        client,
        last_grant,
        ended_at,
    })
}
fn nullable(value: Option<impl ToString>) -> SqlValue {
    value.map_or(SqlValue::Null, text)
}

fn insert_enrollment(next: &StoredEnrollment, at: &str) -> Result<GuardedStatement, StoreFailure> {
    if next.state != StoredEnrollmentState::Approved {
        return Err(corrupt("machine_approvals.state"));
    }
    for (value, field) in [
        (next.id.as_str(), "machine_approvals.approval_id"),
        (next.request_id.as_str(), "machine_approvals.request_id"),
        (
            next.approved_by.as_str(),
            "machine_approvals.approved_by_person_id",
        ),
    ] {
        values::private_id(value, field)?;
    }
    let encoded = state(next)?;
    Ok(exact(
        "INSERT INTO machine_approvals(approval_id,request_id,machine_id,label,allowed_workspaces,state,end_reason,approved_by_person_id,permission_checked,base_generation,code_expires_at,machine_client_id,last_grant_at,ended_at,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        vec![
            text(next.id.as_str()),
            text(next.request_id.as_str()),
            text(next.machine_id),
            text(next.label.as_str()),
            encode(
                &next.allowed_workspaces,
                "machine_approvals.allowed_workspaces",
            )?,
            text(encoded.state),
            nullable(encoded.reason),
            text(next.approved_by.as_str()),
            text("machine:enroll"),
            generation(next.base_generation.get())?,
            text(values::seconds_timestamp(
                next.code_expires_at,
                "machine_approvals.code_expires_at",
            )?),
            nullable(encoded.client),
            nullable(encoded.last_grant),
            nullable(encoded.ended_at),
            text(values::timestamp(
                next.approved_at_ms,
                "machine_approvals.created_at",
            )?),
            text(at),
        ],
    ))
}

fn replace_enrollment(
    before: &StoredEnrollment,
    next: &StoredEnrollment,
    at: &str,
) -> Result<GuardedStatement, StoreFailure> {
    if before.id != next.id
        || before.machine_id != next.machine_id
        || before.installation != next.installation
        || before.request_id != next.request_id
        || before.approved_by != next.approved_by
        || before.label != next.label
        || before.allowed_workspaces != next.allowed_workspaces
        || before.base_generation != next.base_generation
        || before.approved_at_ms != next.approved_at_ms
        || before.code_expires_at != next.code_expires_at
    {
        return Err(corrupt("machine_approvals.immutable_approval"));
    }
    let old = state(before)?;
    let new = state(next)?;
    let last_grant = match &before.state {
        StoredEnrollmentState::Enrolled { last_grant_at, .. } => {
            Some(integer(last_grant_at.0, "machine_approvals.last_grant_at")?)
        }
        _ => None,
    };
    Ok(exact(
        "UPDATE machine_approvals SET state=?,end_reason=?,machine_client_id=?,last_grant_at=?,ended_at=?,updated_at=? WHERE approval_id=? AND machine_id=? AND state=? AND end_reason IS ? AND machine_client_id IS ? AND unixepoch(last_grant_at) IS ? AND ended_at IS NULL AND base_generation=? AND unixepoch(code_expires_at)=? AND request_id=? AND label=? AND json(allowed_workspaces)=json(?) AND approved_by_person_id=? AND permission_checked='machine:enroll' AND unixepoch(created_at)*1000+CAST(substr(strftime('%f',created_at),4,3) AS INTEGER)=?",
        vec![
            text(new.state),
            nullable(new.reason),
            nullable(new.client),
            nullable(new.last_grant),
            nullable(new.ended_at),
            text(at),
            text(before.id.as_str()),
            text(before.machine_id),
            text(old.state),
            nullable(old.reason),
            nullable(old.client),
            last_grant.unwrap_or(SqlValue::Null),
            generation(before.base_generation.get())?,
            integer(
                before.code_expires_at.0,
                "machine_approvals.code_expires_at",
            )?,
            text(before.request_id.as_str()),
            text(before.label.as_str()),
            encode(
                &before.allowed_workspaces,
                "machine_approvals.allowed_workspaces",
            )?,
            text(before.approved_by.as_str()),
            integer(before.approved_at_ms, "machine_approvals.created_at")?,
        ],
    ))
}

fn time_guard(
    before: &StoredEnrollment,
    next: &StoredEnrollment,
    now_ms: i64,
) -> Result<Option<GuardedStatement>, StoreFailure> {
    let now = integer(now_ms, "machine_login.applying_time")?;
    let now_seconds = integer(now_ms.div_euclid(1000), "machine_login.applying_time")?;
    let idle = integer(IDLE_EXPIRY_SECONDS, "machine_login.idle_expiry")?;
    // These no-op guards use the same statement vocabulary as authorization floors.
    // A miss rolls back every earlier effect in this decision's batch.
    let guard = match (&before.state, &next.state) {
        (StoredEnrollmentState::Approved, StoredEnrollmentState::Enrolled { .. }) => exact(
            "UPDATE machine_approvals SET approval_id=approval_id WHERE approval_id=? AND state='approved' AND unixepoch(code_expires_at)*1000>?",
            vec![text(before.id.as_str()), now],
        ),
        (StoredEnrollmentState::Approved, StoredEnrollmentState::Lapsed) => exact(
            "UPDATE machine_approvals SET approval_id=approval_id WHERE approval_id=? AND state='approved' AND unixepoch(code_expires_at)*1000<=?",
            vec![text(before.id.as_str()), now],
        ),
        (StoredEnrollmentState::Enrolled { .. }, StoredEnrollmentState::Enrolled { .. }) => exact(
            "UPDATE machine_approvals SET approval_id=approval_id WHERE approval_id=? AND state='enrolled' AND ?-unixepoch(last_grant_at)<?",
            vec![text(before.id.as_str()), now_seconds, idle],
        ),
        (StoredEnrollmentState::Enrolled { .. }, StoredEnrollmentState::Expired { .. }) => exact(
            "UPDATE machine_approvals SET approval_id=approval_id WHERE approval_id=? AND state='enrolled' AND ?-unixepoch(last_grant_at)>=?",
            vec![text(before.id.as_str()), now_seconds, idle],
        ),
        (StoredEnrollmentState::Enrolled { .. }, StoredEnrollmentState::Revoked { .. }) => {
            return Ok(None);
        }
        _ => return Err(corrupt("machine_approvals.state")),
    };
    Ok(Some(guard))
}

#[cfg(test)]
#[path = "machine_login_writes_test.rs"]
mod tests;
