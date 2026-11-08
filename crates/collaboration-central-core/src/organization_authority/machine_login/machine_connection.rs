//! A machine's connection, read live against its enrollment: admission on
//! every new connection and renewal (PD:596, PD:907-920), and the owner's
//! revocation, which ends the enrollment and tells the host to close the
//! connection (B3's revocation flow).
use super::decision_support::{DecisionIds, DecisionTime, MachineLoginSettings};
use super::enrollment_liveness::idle_expiry;
use super::enrollment_port::{
    Decided, EnrollmentFacts, EnrollmentQuery, MachineEnrollmentFailure, MachineEnrollmentPort,
};
use super::enrollment_records::{
    AllowanceRow, EnrollmentChange, InstallationRow, StoredEnrollment, StoredEnrollmentState,
};
use crate::identity_contract::{
    AdmitMachineConnection, AdmittedMachine, AllowanceState, CommandRefusal,
    EnrollmentRevocationReason, MachineConnectionAdmission, MachineConnectionRefusal, PersonId,
    RevokeMachineEnrollment, RevokeMachineEnrollmentResult,
};
use crate::organization_authority::{
    ChangeRequestId, OrganizationChangeKind, OrganizationChangeOrigin,
};
use fleet_identity_protocol::{EnrollmentId, InstallationId};
use runtime_primitives::{Clock, IdSource};

pub async fn admit_machine_connection<P: MachineEnrollmentPort>(
    port: &P,
    request: &AdmitMachineConnection,
    clock: &dyn Clock,
    settings: MachineLoginSettings,
) -> Result<MachineConnectionAdmission, MachineEnrollmentFailure> {
    let query = EnrollmentQuery {
        enrollment: Some(request.enrollment_id.clone()),
        ..EnrollmentQuery::default()
    };
    port.transact(query, |facts| {
        admit(request, facts, DecisionTime::sample(clock), &settings)
    })
    .await
}

/// A verified token's two upper bounds (PD:578): `now < exp + D`, and
/// `now < grant_expires_at` with no leeway. Both are exclusive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TokenTimeBounds {
    pub exp_with_skew_ms: i64,
    pub grant_expires_at_ms: i64,
}

/// Which bound had passed by the time the admitting transaction read its clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TokenLapse {
    Expired,
    GrantExpired,
}

/// Admission for a token verified before the transaction: its bounds are
/// rechecked on the clock read inside the transaction, so a wait for the
/// port can't carry a token past them.
pub(super) async fn admit_within_token_bounds<P: MachineEnrollmentPort>(
    port: &P,
    request: &AdmitMachineConnection,
    bounds: TokenTimeBounds,
    clock: &dyn Clock,
    settings: MachineLoginSettings,
) -> Result<Result<MachineConnectionAdmission, TokenLapse>, MachineEnrollmentFailure> {
    let query = EnrollmentQuery {
        enrollment: Some(request.enrollment_id.clone()),
        ..EnrollmentQuery::default()
    };
    port.transact(query, |facts| {
        let time = DecisionTime::sample(clock);
        if time.now_ms >= bounds.exp_with_skew_ms {
            return Decided::unchanged(Err(TokenLapse::Expired));
        }
        if time.now_ms >= bounds.grant_expires_at_ms {
            return Decided::unchanged(Err(TokenLapse::GrantExpired));
        }
        let decided = admit(request, facts, time, &settings);
        Decided {
            answer: Ok(decided.answer),
            changes: decided.changes,
        }
    })
    .await
}

fn refused(reason: MachineConnectionRefusal) -> Decided<MachineConnectionAdmission> {
    Decided::unchanged(MachineConnectionAdmission::Refused { reason })
}

fn admit(
    request: &AdmitMachineConnection,
    facts: &EnrollmentFacts,
    time: DecisionTime,
    settings: &MachineLoginSettings,
) -> Decided<MachineConnectionAdmission> {
    let Some(enrollment) = &facts.enrollment else {
        return refused(MachineConnectionRefusal::UnknownMachine {});
    };
    if enrollment.approval.organization_id != request.organization_id {
        return refused(MachineConnectionRefusal::MachineInAnotherOrganization {});
    }
    // The token, the enrollment and the connection must name one machine.
    if request.installation != enrollment.installation
        || request.presented.installation != enrollment.installation
        || request.presented.peer != enrollment.approval.peer
    {
        return refused(MachineConnectionRefusal::UnknownMachine {});
    }
    match &enrollment.state {
        StoredEnrollmentState::Enrolled {
            machine_client_id, ..
        } => {
            let active = facts
                .installation
                .as_ref()
                .and_then(|row| row.active.as_ref());
            if active != Some(&enrollment.id) {
                refused(MachineConnectionRefusal::Superseded {})
            } else if machine_client_id != &request.client_id {
                refused(MachineConnectionRefusal::WrongClient {})
            } else if let Some(expiry) = idle_expiry(enrollment, facts, time, settings) {
                Decided {
                    answer: MachineConnectionAdmission::Refused {
                        reason: MachineConnectionRefusal::EnrollmentNotActive {},
                    },
                    changes: expiry,
                }
            } else {
                Decided::unchanged(MachineConnectionAdmission::Admitted {
                    machine: AdmittedMachine {
                        installation: enrollment.installation.clone(),
                        peer: enrollment.approval.peer,
                        organization_id: request.organization_id.clone(),
                        enrollment_id: enrollment.id.clone(),
                        token_expires_at: request.token_expires_at,
                    },
                })
            }
        }
        StoredEnrollmentState::Revoked {
            reason: EnrollmentRevocationReason::Superseded {},
            ..
        } => refused(MachineConnectionRefusal::Superseded {}),
        StoredEnrollmentState::Approved
        | StoredEnrollmentState::Lapsed
        | StoredEnrollmentState::Revoked { .. }
        | StoredEnrollmentState::Expired { .. } => {
            refused(MachineConnectionRefusal::EnrollmentNotActive {})
        }
    }
}

/// The host closes this machine's open connection once the revocation commits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloseMachineConnection {
    pub installation: InstallationId,
    pub enrollment_id: EnrollmentId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MachineRevocation {
    pub answer: RevokeMachineEnrollmentResult,
    pub close: Option<CloseMachineConnection>,
}

/// Who revoked, and the admin operation's own identity for its records.
#[derive(Clone, Debug)]
pub struct RevocationBy {
    pub person: PersonId,
    pub operation: ChangeRequestId,
}

pub async fn revoke_machine_enrollment<P: MachineEnrollmentPort>(
    port: &P,
    request: &RevokeMachineEnrollment,
    by: &RevocationBy,
    clock: &dyn Clock,
    ids: &dyn IdSource,
) -> Result<MachineRevocation, MachineEnrollmentFailure> {
    let query = EnrollmentQuery {
        installation: Some(request.installation.clone()),
        ..EnrollmentQuery::default()
    };
    port.transact(query, |facts| {
        revoke(facts, by, DecisionTime::sample(clock), &DecisionIds(ids))
            .unwrap_or_else(|failure| Decided::unchanged(Err(failure)))
    })
    .await?
}

/// One transaction: the active enrollment is revoked for its owner, the
/// generation bumped (so no earlier approval can activate), its allowances
/// withdrawn, and the records written.
fn revoke(
    facts: &EnrollmentFacts,
    by: &RevocationBy,
    time: DecisionTime,
    ids: &DecisionIds<'_>,
) -> Result<Decided<Result<MachineRevocation, MachineEnrollmentFailure>>, MachineEnrollmentFailure>
{
    let (Some(row), Some(active)) = (&facts.installation, &facts.active) else {
        return Ok(not_found());
    };
    let StoredEnrollmentState::Enrolled {
        machine_client_id, ..
    } = &active.state
    else {
        return Ok(not_found());
    };
    let Some(ended) = active.ended_client() else {
        return Ok(not_found());
    };
    let origin = OrganizationChangeOrigin::Person {
        person_id: by.person.as_str().to_owned(),
        permission_checked: "machine:revoke".to_owned(),
    };
    let mut changes = vec![
        EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
            state: StoredEnrollmentState::Revoked {
                machine_client_id: machine_client_id.clone(),
                reason: EnrollmentRevocationReason::Owner {},
            },
            ..active.clone()
        }),
        EnrollmentChange::PutInstallation(InstallationRow {
            installation: row.installation.clone(),
            generation: row.generation.next(),
            active: None,
        }),
        EnrollmentChange::Record(ids.record(
            by.operation.as_str(),
            OrganizationChangeKind::MachineRevoked,
            &origin,
            active.id.as_str().to_owned(),
            time,
        )?),
    ];
    for allowance in facts
        .allowances
        .iter()
        .filter(|row| row.state == AllowanceState::Active)
    {
        changes.push(EnrollmentChange::PutAllowance(AllowanceRow {
            state: AllowanceState::Withdrawn,
            ..allowance.clone()
        }));
        changes.push(EnrollmentChange::Record(ids.record(
            by.operation.as_str(),
            OrganizationChangeKind::AllowanceChanged,
            &origin,
            allowance.id.to_string(),
            time,
        )?));
    }
    Ok(Decided {
        answer: Ok(MachineRevocation {
            answer: RevokeMachineEnrollmentResult::Applied { ended },
            close: Some(CloseMachineConnection {
                installation: row.installation.clone(),
                enrollment_id: active.id.clone(),
            }),
        }),
        changes,
    })
}

fn not_found() -> Decided<Result<MachineRevocation, MachineEnrollmentFailure>> {
    Decided::unchanged(Ok(MachineRevocation {
        answer: RevokeMachineEnrollmentResult::Refused {
            reason: CommandRefusal::NotFound {},
        },
        close: None,
    }))
}

#[cfg(test)]
#[path = "machine_connection_test.rs"]
mod tests;
