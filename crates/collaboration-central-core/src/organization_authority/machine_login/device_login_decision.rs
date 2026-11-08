//! `DecideDeviceLogin`: authorize, reserve and approve in one transaction
//! (PD:925-935). An approval admits nothing. Its enrollment becomes active only at
//! its first machine token, and only if no newer login activated first (BA1).
use super::decision_support::{DecisionIds, DecisionTime};
use super::enrollment_port::{
    Decided, EnrollmentFacts, EnrollmentQuery, MachineEnrollmentFailure, MachineEnrollmentPort,
};
use super::enrollment_records::{
    AllowanceRow, EnrollmentChange, EnrollmentGeneration, FormerServiceFacts, InstallationRow,
    MachineClaim, StoredEnrollment, StoredEnrollmentState,
};
use crate::identity_contract::{
    AllowanceState, DecideDeviceLogin, DeviceLoginDecision, DeviceLoginRefusal,
};
use crate::organization_authority::{OrganizationChangeKind, OrganizationChangeOrigin};
use crate::person_request::SECOND_FACTOR_WINDOW_SECONDS;
use fleet_identity_protocol::EnrollmentId;
use runtime_primitives::{Clock, IdSource};
use std::num::NonZeroU32;

/// SD8 (PD:2290): central applies a sign-in decision within five seconds of
/// receiving it, on its own clock. Exactly five seconds is still within.
pub const SD8_MILLIS: i64 = 5_000;

pub async fn decide_device_login<P: MachineEnrollmentPort>(
    port: &P,
    request: &DecideDeviceLogin,
    received_at_ms: i64,
    clock: &dyn Clock,
    ids: &dyn IdSource,
) -> Result<DeviceLoginDecision, MachineEnrollmentFailure> {
    let query = EnrollmentQuery {
        installation: Some(request.installation.clone()),
        peer: Some(request.peer),
        request: Some(request.request_id.clone()),
        former: request.former_service_id.clone(),
        workspaces: request.allowed_workspaces.as_slice().to_vec(),
        enrollment: None,
    };
    port.transact(query, |facts| {
        // The applying clock is read inside the transaction, after any wait.
        let applying = DecisionTime::sample(clock);
        decide(request, facts, received_at_ms, applying, &DecisionIds(ids))
            .unwrap_or_else(|failure| Decided::unchanged(Err(failure)))
    })
    .await?
}

type Answer = Result<DeviceLoginDecision, MachineEnrollmentFailure>;

fn refused(reason: DeviceLoginRefusal) -> Result<Decided<Answer>, MachineEnrollmentFailure> {
    Ok(Decided::unchanged(Ok(DeviceLoginDecision::Refused {
        reason,
    })))
}

fn decide(
    request: &DecideDeviceLogin,
    facts: &EnrollmentFacts,
    received_at_ms: i64,
    applying: DecisionTime,
    ids: &DecisionIds<'_>,
) -> Result<Decided<Answer>, MachineEnrollmentFailure> {
    let now_ms = applying.now_ms;
    if now_ms.saturating_sub(received_at_ms) > SD8_MILLIS {
        return refused(DeviceLoginRefusal::NotPermitted {});
    }
    // Every step is idempotent on the request ID: the same body gets the
    // same answer, and a different body under it reveals nothing.
    if let Some(earlier) = &facts.by_request {
        if &earlier.approval == request {
            return Ok(Decided::unchanged(Ok(DeviceLoginDecision::Approved {
                enrollment_id: earlier.id.clone(),
            })));
        }
        return refused(DeviceLoginRefusal::NotPermitted {});
    }
    if facts.claim == MachineClaim::AnotherOrganization {
        return refused(DeviceLoginRefusal::MachineInAnotherOrganization {});
    }
    if !facts.missing_workspaces.is_empty() {
        return refused(DeviceLoginRefusal::UnknownWorkspace {});
    }
    // The organization's first machine is exempt (PD:882, R53).
    if facts.organization_has_enrollment
        && !second_factor_fresh(request.approver_second_factor_at.map(|at| at.0), now_ms)
    {
        return refused(DeviceLoginRefusal::SecondFactorRequired {
            within_seconds: second_factor_window(),
        });
    }
    let mut changes = Vec::new();
    let mut installation = facts.installation.clone();
    let mut allowances = facts.allowances.clone();
    match (&facts.former, &request.former_service_id) {
        (
            FormerServiceFacts::LinkedTo {
                installation: linked,
            },
            _,
        ) if linked != &request.installation => {
            return refused(DeviceLoginRefusal::FormerServiceLinkedElsewhere {});
        }
        (
            FormerServiceFacts::Unconverted {
                row,
                allowances: former_allowances,
            },
            Some(former),
        ) if row.installation != request.installation => {
            // Converting into an installation that already has rows would
            // merge two machines: not linkable here.
            if facts.installation.is_some() {
                return refused(DeviceLoginRefusal::FormerServiceLinkedElsewhere {});
            }
            changes.push(EnrollmentChange::RekeyInstallation {
                from: row.installation.clone(),
                to: request.installation.clone(),
            });
            changes.push(EnrollmentChange::LinkFormerService {
                former: former.clone(),
                installation: request.installation.clone(),
            });
            installation = Some(InstallationRow {
                installation: request.installation.clone(),
                ..row.clone()
            });
            allowances = former_allowances
                .iter()
                .cloned()
                .map(|allowance| AllowanceRow {
                    installation: request.installation.clone(),
                    ..allowance
                })
                .collect();
        }
        _ => {}
    }
    let base_generation = installation
        .as_ref()
        .map_or_else(EnrollmentGeneration::default, |row| row.generation);
    if installation.is_none() {
        changes.push(EnrollmentChange::PutInstallation(InstallationRow {
            installation: request.installation.clone(),
            generation: base_generation,
            active: None,
        }));
    }
    let enrollment_id = EnrollmentId::try_from(ids.next()?.to_string())
        .map_err(|_| MachineEnrollmentFailure::InvalidIdSource)?;
    changes.push(EnrollmentChange::InsertEnrollment(StoredEnrollment {
        id: enrollment_id.clone(),
        installation: request.installation.clone(),
        approval: request.clone(),
        base_generation,
        approved_at_ms: now_ms,
        state: StoredEnrollmentState::Approved,
    }));
    let origin = OrganizationChangeOrigin::Person {
        person_id: request.approver.as_str().to_owned(),
        permission_checked: "machine:enroll".to_owned(),
    };
    for workspace in request.allowed_workspaces.as_slice() {
        let existing = allowances
            .iter()
            .find(|row| &row.workspace == workspace)
            .cloned();
        if existing
            .as_ref()
            .is_some_and(|row| row.state == AllowanceState::Active)
        {
            continue;
        }
        let allowance = AllowanceRow {
            id: existing.map_or_else(|| ids.next(), |row| Ok(row.id))?,
            installation: request.installation.clone(),
            workspace: workspace.clone(),
            state: AllowanceState::Active,
        };
        changes.push(EnrollmentChange::Record(ids.record(
            request.request_id.as_str(),
            OrganizationChangeKind::AllowanceChanged,
            &origin,
            allowance.id.to_string(),
            applying,
        )?));
        allowances.push(allowance.clone());
        changes.push(EnrollmentChange::PutAllowance(allowance));
    }
    Ok(Decided {
        answer: Ok(DeviceLoginDecision::Approved { enrollment_id }),
        changes,
    })
}

/// A second factor counts for `SECOND_FACTOR_WINDOW_SECONDS` after it was
/// given, never before, as for every other R13 change.
fn second_factor_fresh(second_factor_at: Option<i64>, now_ms: i64) -> bool {
    second_factor_at
        .and_then(|at| at.checked_mul(1000))
        .and_then(|at| now_ms.checked_sub(at))
        .is_some_and(|age_ms| (0..=SECOND_FACTOR_WINDOW_SECONDS * 1000).contains(&age_ms))
}

fn second_factor_window() -> NonZeroU32 {
    u32::try_from(SECOND_FACTOR_WINDOW_SECONDS)
        .ok()
        .and_then(NonZeroU32::new)
        .unwrap_or(NonZeroU32::MIN)
}

#[cfg(test)]
#[path = "device_login_decision_test.rs"]
mod tests;
