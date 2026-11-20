//! Verified token provenance becomes a connection only after live organization admission.
use crate::{
    identity_contract::{ClientId, MachineConnectionRefusal},
    machine_login_row_values::identifier,
    organization_authority::machine_login::{EnrollmentFacts, MachineState, StoredEnrollmentState},
};
use central_storage::StoreFailure;
use fleet_identity_protocol::{EnrollmentId, OrganizationId, UnixSeconds};
use fleet_runtime_ports::workspace_decode_error::corrupt;
use serde::{Deserialize, Serialize};
use workspace_ids::MachineId;

/// Hibernation provenance is untrusted until the runtime restores it against live rows.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MachineConnectionAttachment {
    organization_id: OrganizationId,
    machine_id: MachineId,
    enrollment_id: EnrollmentId,
    client_id: ClientId,
    effective_expires_at: UnixSeconds,
}

impl MachineConnectionAttachment {
    pub(crate) fn from_verified(verified: &crate::machine_token::VerifiedMachineToken) -> Self {
        Self {
            organization_id: verified.organization_id().clone(),
            machine_id: verified.machine_id().clone(),
            enrollment_id: verified.enrollment_id().clone(),
            client_id: verified.client_id().clone(),
            effective_expires_at: verified.effective_expires_at(),
        }
    }
    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        &self.organization_id
    }
    pub(crate) fn machine_id(&self) -> &MachineId {
        &self.machine_id
    }
    pub(crate) fn enrollment_id(&self) -> &EnrollmentId {
        &self.enrollment_id
    }
}

/// Core-created connection provenance. It carries no cached workspace permissions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedMachineConnection {
    attachment: MachineConnectionAttachment,
}
impl ValidatedMachineConnection {
    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        &self.attachment.organization_id
    }
    #[must_use]
    pub fn machine_id(&self) -> &MachineId {
        &self.attachment.machine_id
    }
    #[must_use]
    pub fn enrollment_id(&self) -> &EnrollmentId {
        &self.attachment.enrollment_id
    }
    #[must_use]
    pub fn client_id(&self) -> &ClientId {
        &self.attachment.client_id
    }
    #[must_use]
    pub fn effective_expires_at(&self) -> UnixSeconds {
        self.attachment.effective_expires_at
    }
    #[must_use]
    pub fn attachment(&self) -> MachineConnectionAttachment {
        self.attachment.clone()
    }
}

pub(crate) type Admission =
    Result<Result<ValidatedMachineConnection, MachineConnectionRefusal>, StoreFailure>;

pub(crate) fn validate_shape(attachment: &MachineConnectionAttachment) -> Result<(), StoreFailure> {
    let _: OrganizationId = identifier(
        attachment.organization_id.as_str(),
        "connection.organization_id",
    )?;
    let _: EnrollmentId = identifier(
        attachment.enrollment_id.as_str(),
        "connection.enrollment_id",
    )?;
    let _: ClientId = identifier(attachment.client_id.as_str(), "connection.client_id")?;
    if attachment.effective_expires_at.0 < 0 {
        return Err(corrupt("connection.effective_expires_at"));
    }
    attachment
        .effective_expires_at
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
    if &attachment.organization_id != organization || &facts.organization_id != organization {
        return Ok(Err(
            MachineConnectionRefusal::MachineInAnotherOrganization {},
        ));
    }
    if now_ms.div_euclid(1000) >= attachment.effective_expires_at.0 {
        return Ok(Err(MachineConnectionRefusal::EnrollmentNotActive {}));
    }
    let Some(machine) = &facts.machine else {
        return Ok(Err(MachineConnectionRefusal::UnknownMachine {}));
    };
    let Some(enrollment) = &facts.enrollment else {
        return Ok(Err(MachineConnectionRefusal::EnrollmentNotActive {}));
    };
    if machine.machine_id != attachment.machine_id
        || enrollment.machine_id != attachment.machine_id
        || enrollment.id != attachment.enrollment_id
        || machine.state != MachineState::Enrolled
    {
        return Ok(Err(MachineConnectionRefusal::EnrollmentNotActive {}));
    }
    if facts.active.as_ref().map(|active| &active.id) != Some(&enrollment.id) {
        return Ok(Err(MachineConnectionRefusal::Superseded {}));
    }
    if attachment.client_id.as_str() != attachment.enrollment_id.as_str() {
        return Ok(Err(MachineConnectionRefusal::WrongClient {}));
    }
    match &enrollment.state {
        StoredEnrollmentState::Enrolled { client_id, .. } if client_id == &attachment.client_id => {
        }
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
    Ok(Ok(ValidatedMachineConnection {
        attachment: attachment.clone(),
    }))
}

#[cfg(test)]
#[path = "machine_connection_test.rs"]
mod tests;
