//! Approval reserves a snapshot; it never activates a login.
use super::decision_support::{DecisionIds, DecisionTime};
use super::enrollment_records::{
    Decided, EnrollmentChange, EnrollmentFacts, EnrollmentGeneration, MachineRow, MachineState,
    StoredEnrollment, StoredEnrollmentState,
};
use crate::identity_contract::{DecideDeviceLogin, DeviceLoginDecision, DeviceLoginRefusal};
use crate::organization_change_rows::{OrganizationChangeKind, OrganizationChangeOrigin};
use crate::person_request::SECOND_FACTOR_WINDOW_SECONDS;
use central_storage::StoreFailure;
use fleet_identity_protocol::EnrollmentId;
use runtime_primitives::IdSource;
use std::num::NonZeroU32;

pub(crate) const SD8_MILLIS: i64 = 5_000;

pub(crate) fn decide_device_login(
    request: &DecideDeviceLogin,
    facts: &EnrollmentFacts,
    received_at_ms: i64,
    time: DecisionTime,
    ids: &dyn IdSource,
) -> Result<Decided<DeviceLoginDecision>, StoreFailure> {
    let refuse = |reason| Ok(Decided::unchanged(DeviceLoginDecision::Refused { reason }));
    if request.organization_id != facts.organization_id
        || time.now_ms.saturating_sub(received_at_ms) > SD8_MILLIS
    {
        return refuse(DeviceLoginRefusal::NotPermitted {});
    }
    let machine_id = request.machine_id.clone();
    // N03: after admission and SD8, request identity owns the first snapshot
    // for its original machine_id.
    // Mutable labels, workspaces, factor freshness and enrollment state cannot
    // replace that answer or create another effect.
    if let Some(earlier) = &facts.prior_approval {
        if earlier.machine_id != machine_id {
            return refuse(DeviceLoginRefusal::NotPermitted {});
        }
        return Ok(Decided::unchanged(approved(earlier)));
    }
    let Some(deadline_ms) = request.code_expires_at.0.checked_mul(1000) else {
        return refuse(DeviceLoginRefusal::NotPermitted {});
    };
    if deadline_ms <= time.now_ms {
        return refuse(DeviceLoginRefusal::NotPermitted {});
    }
    if !facts.missing_workspaces.is_empty() {
        return refuse(DeviceLoginRefusal::UnknownWorkspace {});
    }
    if facts.organization_has_enrollment
        && !second_factor_fresh(
            request.approver_second_factor_at.map(|at| at.0),
            time.now_ms,
        )
    {
        let within_seconds = u32::try_from(SECOND_FACTOR_WINDOW_SECONDS)
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(StoreFailure::Unavailable)?;
        return refuse(DeviceLoginRefusal::SecondFactorRequired { within_seconds });
    }
    let ids = DecisionIds(ids);
    let mut machine = match &facts.machine {
        Some(machine) => machine.clone(),
        None => MachineRow {
            machine_id: machine_id.clone(),
            label: request.label.clone(),
            allowed_workspaces: Vec::new(),
            generation: EnrollmentGeneration::default(),
            state: MachineState::Revoked,
        },
    };
    machine.label = request.label.clone();
    // Approval adds allowances; it does not withdraw existing ones (B3/R39).
    let added: Vec<_> = request
        .allowed_workspaces
        .as_slice()
        .iter()
        .filter(|workspace| !machine.allowed_workspaces.contains(workspace))
        .cloned()
        .collect();
    for workspace in &added {
        if !machine.allowed_workspaces.contains(workspace) {
            machine.allowed_workspaces.push(workspace.clone());
        }
    }
    let enrollment = StoredEnrollment {
        id: EnrollmentId::try_from(ids.next()?.to_string())
            .map_err(|_| StoreFailure::Unavailable)?,
        machine_id,
        request_id: request.request_id.clone(),
        approved_by: request.approver.clone(),
        label: request.label.clone(),
        allowed_workspaces: request.allowed_workspaces.clone(),
        base_generation: machine.generation,
        approved_at_ms: time.now_ms,
        code_expires_at: request.code_expires_at,
        state: StoredEnrollmentState::Approved,
        ended_at_ms: None,
    };
    // The owning machine must precede its approval's foreign key.
    let mut changes = vec![
        EnrollmentChange::PutMachine(machine),
        EnrollmentChange::InsertEnrollment(enrollment.clone()),
    ];
    let origin = OrganizationChangeOrigin::Person {
        person_id: request.approver.as_str().to_owned(),
        permission_checked: "machine:enroll".to_owned(),
    };
    let mut recorded = Vec::new();
    for workspace in added {
        if recorded.contains(&workspace) {
            continue;
        }
        changes.push(EnrollmentChange::Record(ids.record(
            request.request_id.as_str(),
            OrganizationChangeKind::AllowanceChanged,
            &origin,
            workspace.as_str().to_owned(),
            time,
        )?));
        recorded.push(workspace);
    }
    Ok(Decided::writing(
        approved(&enrollment),
        changes,
        time.now_ms,
    ))
}

fn approved(enrollment: &StoredEnrollment) -> DeviceLoginDecision {
    DeviceLoginDecision::Approved {
        enrollment_id: enrollment.id.clone(),
        label: enrollment.label.clone(),
        allowed_workspaces: enrollment.allowed_workspaces.clone(),
    }
}
fn second_factor_fresh(second_factor_at: Option<i64>, now_ms: i64) -> bool {
    second_factor_at
        .and_then(|at| at.checked_mul(1000))
        .and_then(|at| now_ms.checked_sub(at))
        .is_some_and(|age| (0..=SECOND_FACTOR_WINDOW_SECONDS * 1000).contains(&age))
}

#[cfg(test)]
#[path = "device_login_decision_test.rs"]
mod tests;
