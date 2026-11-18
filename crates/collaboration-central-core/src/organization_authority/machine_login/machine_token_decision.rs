//! Activation is generation-fenced; renewal rechecks live client and idle state.
use super::decision_support::{
    DecisionIds, DecisionTime, IDLE_EXPIRY_SECONDS, MACHINE_GRANT_LIFETIME_SECONDS,
};
use super::enrollment_records::{
    Decided, EnrollmentChange, EnrollmentFacts, MachineRow, MachineState, StoredEnrollment,
    StoredEnrollmentState,
};
use crate::identity_contract::{
    DecideMachineToken, EndedMachineClient, EnrollmentRevocationReason, MachineTokenDecision,
    MachineTokenRefusal,
};
use crate::organization_change_rows::{OrganizationChangeKind, OrganizationChangeOrigin};
use central_storage::StoreFailure;
use fleet_identity_protocol::UnixSeconds;
use fleet_runtime_ports::IdSource;

pub(crate) fn decide_machine_token(
    request: &DecideMachineToken,
    facts: &EnrollmentFacts,
    time: DecisionTime,
    ids: &dyn IdSource,
) -> Result<Decided<MachineTokenDecision>, StoreFailure> {
    if request.organization_id != facts.organization_id {
        return Ok(refuse(MachineTokenRefusal::EnrollmentNotActive {}));
    }
    let Some(enrollment) = &facts.enrollment else {
        return Ok(refuse(MachineTokenRefusal::EnrollmentNotActive {}));
    };
    let ids = DecisionIds(ids);
    match &enrollment.state {
        StoredEnrollmentState::Approved => first_grant(request, enrollment, facts, time, &ids),
        StoredEnrollmentState::Enrolled {
            client_id,
            last_grant_at,
        } => {
            if facts.active.as_ref().map(|active| &active.id) != Some(&enrollment.id) {
                return Ok(refuse(MachineTokenRefusal::Superseded {}));
            }
            if client_id != &request.client_id {
                return Ok(refuse(MachineTokenRefusal::WrongClient {}));
            }
            if time.seconds().0.saturating_sub(last_grant_at.0) >= IDLE_EXPIRY_SECONDS {
                return idle_expiry(enrollment, facts, time, &ids);
            }
            Ok(Decided::writing(
                grant(enrollment, time, facts.superseded_client.clone())?,
                vec![EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
                    state: StoredEnrollmentState::Enrolled {
                        client_id: client_id.clone(),
                        last_grant_at: time.seconds(),
                    },
                    ..enrollment.clone()
                })],
                time.now_ms,
            ))
        }
        StoredEnrollmentState::Revoked {
            reason: EnrollmentRevocationReason::Superseded {},
            ..
        } => Ok(refuse(MachineTokenRefusal::Superseded {})),
        StoredEnrollmentState::Lapsed
        | StoredEnrollmentState::Revoked { .. }
        | StoredEnrollmentState::Expired { .. } => {
            Ok(refuse(MachineTokenRefusal::EnrollmentNotActive {}))
        }
    }
}

fn first_grant(
    request: &DecideMachineToken,
    enrollment: &StoredEnrollment,
    facts: &EnrollmentFacts,
    time: DecisionTime,
    ids: &DecisionIds<'_>,
) -> Result<Decided<MachineTokenDecision>, StoreFailure> {
    let deadline = enrollment
        .code_expires_at
        .0
        .checked_mul(1000)
        .ok_or(StoreFailure::Unavailable)?;
    if time.now_ms >= deadline {
        return Ok(Decided::writing(
            MachineTokenDecision::Refuse {
                reason: MachineTokenRefusal::EnrollmentNotActive {},
            },
            vec![EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
                state: StoredEnrollmentState::Lapsed,
                ended_at_ms: Some(time.now_ms),
                ..enrollment.clone()
            })],
            time.now_ms,
        ));
    }
    if request.client_id.as_str() != enrollment.id.as_str() {
        return Ok(refuse(MachineTokenRefusal::WrongClient {}));
    }
    let machine = facts.machine.as_ref().ok_or(StoreFailure::Unavailable)?;
    if machine.generation != enrollment.base_generation {
        return Ok(refuse(MachineTokenRefusal::Superseded {}));
    }
    let next_generation = machine.generation.next()?;
    let origin = OrganizationChangeOrigin::Person {
        person_id: enrollment.approved_by.as_str().to_owned(),
        permission_checked: "machine:enroll".to_owned(),
    };
    let mut changes = Vec::new();
    let superseded = facts
        .active
        .as_ref()
        .and_then(|previous| previous.ended_client());
    if let Some(previous) = &facts.active {
        let StoredEnrollmentState::Enrolled { client_id, .. } = &previous.state else {
            return Err(StoreFailure::Unavailable);
        };
        // End the old enrollment before entering the partial unique index.
        changes.push(EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
            state: StoredEnrollmentState::Revoked {
                client_id: client_id.clone(),
                reason: EnrollmentRevocationReason::Superseded {},
            },
            ended_at_ms: Some(time.now_ms),
            ..previous.clone()
        }));
        changes.push(EnrollmentChange::Record(ids.record(
            enrollment.request_id.as_str(),
            OrganizationChangeKind::MachineRevoked,
            &origin,
            previous.id.as_str().to_owned(),
            time,
        )?));
    }
    changes.push(EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
        state: StoredEnrollmentState::Enrolled {
            client_id: request.client_id.clone(),
            last_grant_at: time.seconds(),
        },
        ..enrollment.clone()
    }));
    changes.push(EnrollmentChange::PutMachine(MachineRow {
        generation: next_generation,
        state: MachineState::Enrolled,
        ..machine.clone()
    }));
    changes.push(EnrollmentChange::Record(ids.record(
        enrollment.request_id.as_str(),
        OrganizationChangeKind::MachineEnrolled,
        &origin,
        enrollment.id.as_str().to_owned(),
        time,
    )?));
    Ok(Decided::writing(
        grant(enrollment, time, superseded)?,
        changes,
        time.now_ms,
    ))
}

fn grant(
    enrollment: &StoredEnrollment,
    time: DecisionTime,
    superseded: Option<EndedMachineClient>,
) -> Result<MachineTokenDecision, StoreFailure> {
    let decided_at = time.seconds();
    let expires = decided_at
        .0
        .checked_add(MACHINE_GRANT_LIFETIME_SECONDS)
        .ok_or(StoreFailure::Unavailable)?;
    Ok(MachineTokenDecision::Grant {
        installation: enrollment.installation.clone(),
        enrollment_id: enrollment.id.clone(),
        decided_at,
        grant_expires_at: UnixSeconds(expires),
        superseded,
    })
}

fn idle_expiry(
    enrollment: &StoredEnrollment,
    facts: &EnrollmentFacts,
    time: DecisionTime,
    ids: &DecisionIds<'_>,
) -> Result<Decided<MachineTokenDecision>, StoreFailure> {
    let StoredEnrollmentState::Enrolled { client_id, .. } = &enrollment.state else {
        return Err(StoreFailure::Unavailable);
    };
    let machine = facts.machine.as_ref().ok_or(StoreFailure::Unavailable)?;
    let automatic = OrganizationChangeOrigin::Automatic {};
    let mut changes = vec![
        EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
            state: StoredEnrollmentState::Expired {
                client_id: client_id.clone(),
            },
            ended_at_ms: Some(time.now_ms),
            ..enrollment.clone()
        }),
        EnrollmentChange::PutMachine(MachineRow {
            generation: machine.generation.next()?,
            state: MachineState::Revoked,
            allowed_workspaces: Vec::new(),
            ..machine.clone()
        }),
        EnrollmentChange::Record(ids.record(
            enrollment.id.as_str(),
            OrganizationChangeKind::MachineExpired,
            &automatic,
            enrollment.id.as_str().to_owned(),
            time,
        )?),
    ];
    append_allowance_records(
        &mut changes,
        machine,
        enrollment.id.as_str(),
        &automatic,
        time,
        ids,
    )?;
    Ok(Decided::writing(
        MachineTokenDecision::Refuse {
            reason: MachineTokenRefusal::IdleExpired {},
        },
        changes,
        time.now_ms,
    ))
}

pub(super) fn append_allowance_records(
    changes: &mut Vec<EnrollmentChange>,
    machine: &MachineRow,
    originating: &str,
    origin: &OrganizationChangeOrigin,
    time: DecisionTime,
    ids: &DecisionIds<'_>,
) -> Result<(), StoreFailure> {
    let mut recorded = Vec::new();
    for workspace in &machine.allowed_workspaces {
        if recorded.contains(workspace) {
            continue;
        }
        changes.push(EnrollmentChange::Record(ids.record(
            originating,
            OrganizationChangeKind::AllowanceChanged,
            origin,
            workspace.as_str().to_owned(),
            time,
        )?));
        recorded.push(workspace.clone());
    }
    Ok(())
}
fn refuse(reason: MachineTokenRefusal) -> Decided<MachineTokenDecision> {
    Decided::unchanged(MachineTokenDecision::Refuse { reason })
}

#[cfg(test)]
#[path = "machine_token_decision_test.rs"]
mod tests;
