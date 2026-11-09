//! `DecideMachineToken`, asked before every machine-token mint. The first
//! Grant activates an approval only if the installation's generation is still
//! the one it was approved at (BA1); every later Grant re-reads the enrollment
//! live. Each Grant moves the idle-expiry clock forward.
use super::decision_support::{DecisionIds, DecisionTime, MachineLoginSettings};
use super::enrollment_liveness::{approval_lapse, idle_expiry};
use super::enrollment_port::{
    Decided, EnrollmentFacts, EnrollmentQuery, MachineEnrollmentFailure, MachineEnrollmentPort,
};
use super::enrollment_records::{
    EnrollmentChange, InstallationRow, StoredEnrollment, StoredEnrollmentState,
};
use crate::identity_contract::{
    DecideMachineToken, EnrollmentRevocationReason, MachineTokenDecision, MachineTokenRefusal,
};
use crate::organization_authority::{OrganizationChangeKind, OrganizationChangeOrigin};
use fleet_identity_protocol::UnixSeconds;
use runtime_primitives::{Clock, IdSource};
use std::sync::Arc;

pub async fn decide_machine_token<P: MachineEnrollmentPort>(
    port: &P,
    request: DecideMachineToken,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdSource>,
    settings: MachineLoginSettings,
) -> Result<MachineTokenDecision, MachineEnrollmentFailure> {
    let query = EnrollmentQuery {
        enrollment: Some(request.enrollment_id.clone()),
        ..EnrollmentQuery::default()
    };
    port.transact(query, move |facts| {
        let time = DecisionTime::sample(clock.as_ref());
        decide(&request, facts, time, &DecisionIds(ids.as_ref()), &settings)
            .unwrap_or_else(|failure| Decided::unchanged(Err(failure)))
    })
    .await?
}

type Answer = Result<MachineTokenDecision, MachineEnrollmentFailure>;

fn refuse(reason: MachineTokenRefusal) -> Decided<Answer> {
    Decided::unchanged(Ok(MachineTokenDecision::Refuse { reason }))
}

fn refuse_with(
    reason: MachineTokenRefusal,
    changes: Vec<EnrollmentChange>,
    time: DecisionTime,
) -> Decided<Answer> {
    Decided::writing(
        Ok(MachineTokenDecision::Refuse { reason }),
        changes,
        time.now_ms,
    )
}

fn decide(
    request: &DecideMachineToken,
    facts: &EnrollmentFacts,
    time: DecisionTime,
    ids: &DecisionIds<'_>,
    settings: &MachineLoginSettings,
) -> Result<Decided<Answer>, MachineEnrollmentFailure> {
    let Some(enrollment) = &facts.enrollment else {
        return Ok(refuse(MachineTokenRefusal::EnrollmentNotActive {}));
    };
    Ok(match &enrollment.state {
        StoredEnrollmentState::Approved => {
            first_grant(request, enrollment, facts, time, ids, settings)?
        }
        StoredEnrollmentState::Enrolled {
            machine_client_id, ..
        } => {
            let active = facts
                .installation
                .as_ref()
                .and_then(|row| row.active.as_ref());
            if active != Some(&enrollment.id) {
                refuse(MachineTokenRefusal::Superseded {})
            } else if machine_client_id != &request.client_id {
                refuse(MachineTokenRefusal::WrongClient {})
            } else if let Some(expiry) = idle_expiry(enrollment, facts, time, settings, ids)? {
                refuse_with(MachineTokenRefusal::IdleExpired {}, expiry, time)
            } else {
                Decided::writing(
                    Ok(grant(enrollment, time, settings, None)),
                    vec![EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
                        state: StoredEnrollmentState::Enrolled {
                            machine_client_id: machine_client_id.clone(),
                            last_grant_at: time.seconds(),
                        },
                        ..enrollment.clone()
                    })],
                    time.now_ms,
                )
            }
        }
        StoredEnrollmentState::Revoked {
            reason: EnrollmentRevocationReason::Superseded {},
            ..
        } => refuse(MachineTokenRefusal::Superseded {}),
        StoredEnrollmentState::Lapsed
        | StoredEnrollmentState::Revoked { .. }
        | StoredEnrollmentState::Expired { .. } => {
            refuse(MachineTokenRefusal::EnrollmentNotActive {})
        }
    })
}

/// BA1: activate only if no other enrollment activated, and no revocation
/// happened, since this approval. Then supersede the working enrollment and
/// bump the generation, in this one transaction.
fn first_grant(
    request: &DecideMachineToken,
    enrollment: &StoredEnrollment,
    facts: &EnrollmentFacts,
    time: DecisionTime,
    ids: &DecisionIds<'_>,
    settings: &MachineLoginSettings,
) -> Result<Decided<Answer>, MachineEnrollmentFailure> {
    if let Some(lapse) = approval_lapse(enrollment, time) {
        return Ok(refuse_with(
            MachineTokenRefusal::EnrollmentNotActive {},
            vec![lapse],
            time,
        ));
    }
    // The enrollment's own machine client has the enrollment's ID (PD:956).
    if request.client_id.as_str() != enrollment.id.as_str() {
        return Ok(refuse(MachineTokenRefusal::WrongClient {}));
    }
    let Some(installation) = &facts.installation else {
        return Err(MachineEnrollmentFailure::ContractViolated {
            rule: "an approved enrollment's installation has a row",
        });
    };
    if installation.generation != enrollment.base_generation {
        return Ok(refuse(MachineTokenRefusal::Superseded {}));
    }
    let origin = OrganizationChangeOrigin::Person {
        person_id: enrollment.approved_by.as_str().to_owned(),
        permission_checked: "machine:enroll".to_owned(),
    };
    let request_id = enrollment.request_id.as_str();
    let mut changes = Vec::new();
    let superseded = facts.active.as_ref().and_then(|previous| {
        let StoredEnrollmentState::Enrolled {
            machine_client_id, ..
        } = &previous.state
        else {
            return None;
        };
        Some((previous, machine_client_id.clone()))
    });
    if let Some((previous, machine_client_id)) = &superseded {
        changes.push(EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
            state: StoredEnrollmentState::Revoked {
                machine_client_id: machine_client_id.clone(),
                reason: EnrollmentRevocationReason::Superseded {},
            },
            ended_at_ms: Some(time.now_ms),
            ..(*previous).clone()
        }));
        changes.push(EnrollmentChange::Record(ids.record(
            request_id,
            OrganizationChangeKind::MachineRevoked,
            &origin,
            previous.id.as_str().to_owned(),
            time,
        )?));
    }
    changes.push(EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
        state: StoredEnrollmentState::Enrolled {
            machine_client_id: request.client_id.clone(),
            last_grant_at: time.seconds(),
        },
        ..enrollment.clone()
    }));
    changes.push(EnrollmentChange::PutInstallation(InstallationRow {
        generation: installation.generation.next(),
        active: Some(enrollment.id.clone()),
        ..installation.clone()
    }));
    changes.push(EnrollmentChange::Record(ids.record(
        request_id,
        OrganizationChangeKind::MachineEnrolled,
        &origin,
        enrollment.id.as_str().to_owned(),
        time,
    )?));
    let ended = superseded.and_then(|(previous, _)| previous.ended_client());
    Ok(Decided::writing(
        Ok(grant(enrollment, time, settings, ended)),
        changes,
        time.now_ms,
    ))
}

fn grant(
    enrollment: &StoredEnrollment,
    time: DecisionTime,
    settings: &MachineLoginSettings,
    superseded: Option<crate::identity_contract::EndedMachineClient>,
) -> MachineTokenDecision {
    let decided_at = time.seconds();
    MachineTokenDecision::Grant {
        installation: enrollment.installation.clone(),
        enrollment_id: enrollment.id.clone(),
        decided_at,
        grant_expires_at: UnixSeconds(
            decided_at
                .0
                .saturating_add(i64::from(u32::from(settings.grant_lifetime))),
        ),
        superseded,
    }
}

#[cfg(test)]
#[path = "machine_token_decision_test.rs"]
mod tests;
