use crate::{ClientId, MachineConnectionRefusal};
use central_storage::StoreFailure;
use fleet_identity_protocol::{EnrollmentId, OrganizationId, UnixSeconds};
use serde::{Deserialize, Serialize};
use std::future::Future;
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
    /// Builds untrusted provenance from the token claims.
    pub fn from_token_claims(
        organization_id: OrganizationId,
        machine_id: MachineId,
        enrollment_id: EnrollmentId,
        client_id: ClientId,
        effective_expires_at: UnixSeconds,
    ) -> Self {
        Self {
            organization_id,
            machine_id,
            enrollment_id,
            client_id,
            effective_expires_at,
        }
    }

    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        &self.organization_id
    }
    #[must_use]
    pub fn machine_id(&self) -> &MachineId {
        &self.machine_id
    }
    #[must_use]
    pub fn enrollment_id(&self) -> &EnrollmentId {
        &self.enrollment_id
    }

    #[must_use]
    pub fn client_id(&self) -> &ClientId {
        &self.client_id
    }
    #[must_use]
    pub fn effective_expires_at(&self) -> UnixSeconds {
        self.effective_expires_at
    }
}

/// Core-created connection provenance. It carries no cached workspace permissions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedMachineConnection {
    attachment: MachineConnectionAttachment,
}
impl ValidatedMachineConnection {
    /// Requires successful live admission under the organization turn.
    pub fn from_live_admission(attachment: MachineConnectionAttachment) -> Self {
        Self { attachment }
    }

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

/// Restores the attachment against live login rows under the turn held by the caller.
pub trait LiveMachineConnection: Send + Sync {
    fn restore_held(
        &self,
        attachment: &MachineConnectionAttachment,
    ) -> impl Future<Output = Result<Result<(), MachineConnectionRefusal>, StoreFailure>> + Send;
}
