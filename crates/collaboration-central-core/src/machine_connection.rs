//! Verified token provenance becomes a connection only after live organization admission.
use crate::{
    machine_login_row_values::identifier,
    organization_authority::machine_login::{EnrollmentFacts, MachineState, StoredEnrollmentState},
};
use central_storage::StoreFailure;
use fleet_contracts::{
    ClientId, MachineConnectionAttachment, MachineConnectionRefusal, ValidatedMachineConnection,
};
use fleet_identity_protocol::{EnrollmentId, OrganizationId};
use fleet_runtime_ports::workspace_decode_error::corrupt;

pub(crate) type Admission =
    Result<Result<ValidatedMachineConnection, MachineConnectionRefusal>, StoreFailure>;

pub(crate) fn validate_shape(attachment: &MachineConnectionAttachment) -> Result<(), StoreFailure> {
    let _: OrganizationId = identifier(
        attachment.organization_id().as_str(),
        "connection.organization_id",
    )?;
    let _: EnrollmentId = identifier(
        attachment.enrollment_id().as_str(),
        "connection.enrollment_id",
    )?;
    let _: ClientId = identifier(attachment.client_id().as_str(), "connection.client_id")?;
    if attachment.effective_expires_at().0 < 0 {
        return Err(corrupt("connection.effective_expires_at"));
    }
    attachment
        .effective_expires_at()
        .0
        .checked_mul(1000)
        .and_then(|millis| central_storage::SafeInteger::new(millis).ok())
        .ok_or_else(|| corrupt("connection.effective_expires_at"))?;
    Ok(())
}

pub(crate) fn validate_live(
    attachment: &MachineConnectionAttachment,
    organization: &OrganizationId,
    facts: &EnrollmentFacts,
    now_ms: i64,
) -> Admission {
    validate_shape(attachment)?;
    if attachment.organization_id() != organization || &facts.organization_id != organization {
        return Ok(Err(
            MachineConnectionRefusal::MachineInAnotherOrganization {},
        ));
    }
    if now_ms.div_euclid(1000) >= attachment.effective_expires_at().0 {
        return Ok(Err(MachineConnectionRefusal::EnrollmentNotActive {}));
    }
    let Some(machine) = &facts.machine else {
        return Ok(Err(MachineConnectionRefusal::UnknownMachine {}));
    };
    let Some(enrollment) = &facts.enrollment else {
        return Ok(Err(MachineConnectionRefusal::EnrollmentNotActive {}));
    };
    if &machine.machine_id != attachment.machine_id()
        || &enrollment.machine_id != attachment.machine_id()
        || &enrollment.id != attachment.enrollment_id()
        || machine.state != MachineState::Enrolled
    {
        return Ok(Err(MachineConnectionRefusal::EnrollmentNotActive {}));
    }
    if facts.active.as_ref().map(|active| &active.id) != Some(&enrollment.id) {
        return Ok(Err(MachineConnectionRefusal::Superseded {}));
    }
    if attachment.client_id().as_str() != attachment.enrollment_id().as_str() {
        return Ok(Err(MachineConnectionRefusal::WrongClient {}));
    }
    match &enrollment.state {
        StoredEnrollmentState::Enrolled { client_id, .. }
            if client_id == attachment.client_id() => {}
        StoredEnrollmentState::Enrolled { .. } => {
            return Ok(Err(MachineConnectionRefusal::WrongClient {}));
        }
        StoredEnrollmentState::Approved
        | StoredEnrollmentState::Lapsed
        | StoredEnrollmentState::Revoked { .. }
        | StoredEnrollmentState::Expired { .. } => {
            return Ok(Err(MachineConnectionRefusal::EnrollmentNotActive {}));
        }
    }
    Ok(Ok(ValidatedMachineConnection::from_live_admission(
        attachment.clone(),
    )))
}

#[cfg(test)]
#[path = "machine_connection_test.rs"]
mod tests;
