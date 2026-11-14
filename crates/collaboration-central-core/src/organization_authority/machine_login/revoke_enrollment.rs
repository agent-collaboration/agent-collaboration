//! Private revocation policy; routing and connection close delivery belong to C.
use super::decision_support::{DecisionIds, DecisionTime};
use super::enrollment_records::*;
use super::machine_token_decision::append_allowance_records;
use crate::identity_contract::{EndedMachineClient, EnrollmentRevocationReason, PersonId};
use crate::organization_change_rows::{OrganizationChangeKind, OrganizationChangeOrigin};
use crate::person_request::AdminOperationId;
use central_storage::StoreFailure;
use runtime_primitives::IdSource;

/// None means the installation has no currently enrolled login.
pub(crate) fn revoke_machine_enrollment(
    facts: &EnrollmentFacts,
    person: &PersonId,
    operation: &AdminOperationId,
    time: DecisionTime,
    ids: &dyn IdSource,
) -> Result<Decided<Option<EndedMachineClient>>, StoreFailure> {
    let (Some(machine), Some(active)) = (&facts.machine, &facts.active) else {
        return Ok(Decided::unchanged(None));
    };
    let StoredEnrollmentState::Enrolled { client_id, .. } = &active.state else {
        return Ok(Decided::unchanged(None));
    };
    let next_generation = machine.generation.next()?;
    let ids = DecisionIds(ids);
    let origin = OrganizationChangeOrigin::Person {
        person_id: person.as_str().to_owned(),
        permission_checked: "machine:revoke".to_owned(),
    };
    let mut changes = vec![
        EnrollmentChange::ReplaceEnrollment(StoredEnrollment {
            state: StoredEnrollmentState::Revoked {
                client_id: client_id.clone(),
                reason: EnrollmentRevocationReason::Owner {},
            },
            ended_at_ms: Some(time.now_ms),
            ..active.clone()
        }),
        EnrollmentChange::PutMachine(MachineRow {
            generation: next_generation,
            state: MachineState::Revoked,
            allowed_workspaces: Vec::new(),
            ..machine.clone()
        }),
        EnrollmentChange::Record(ids.record(
            operation.as_str(),
            OrganizationChangeKind::MachineRevoked,
            &origin,
            active.id.as_str().to_owned(),
            time,
        )?),
    ];
    append_allowance_records(
        &mut changes,
        machine,
        operation.as_str(),
        &origin,
        time,
        &ids,
    )?;
    Ok(Decided::writing(
        active.ended_client(),
        changes,
        time.now_ms,
    ))
}

#[cfg(test)]
#[path = "revoke_enrollment_test.rs"]
mod tests;
