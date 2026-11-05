//! A Fleet object is unavailable to public callers until provisioning stamps it.
use fleet_identity_protocol::OrganizationId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrganizationIdentity {
    Unprovisioned,
    Provisioned { organization_id: OrganizationId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvisioningStampAction {
    Initialize,
    AlreadyProvisioned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OrganizationIdentityRefusal {
    #[error("the Fleet object has not been provisioned")]
    Unprovisioned,
    #[error("the request does not belong to this Fleet object")]
    OrganizationMismatch,
}

impl OrganizationIdentity {
    /// The serialized provisioning transaction decides before writing any stamp or record.
    pub fn provisioning_action(
        &self,
        requested_organization: &OrganizationId,
    ) -> Result<ProvisioningStampAction, OrganizationIdentityRefusal> {
        match self {
            Self::Unprovisioned => Ok(ProvisioningStampAction::Initialize),
            Self::Provisioned { organization_id } if organization_id == requested_organization => {
                Ok(ProvisioningStampAction::AlreadyProvisioned)
            }
            Self::Provisioned { .. } => Err(OrganizationIdentityRefusal::OrganizationMismatch),
        }
    }

    /// Run before reading a public request's subject; refusals carry no stored identity.
    pub fn require_matching_organization(
        &self,
        requested_organization: &OrganizationId,
    ) -> Result<(), OrganizationIdentityRefusal> {
        match self {
            Self::Unprovisioned => Err(OrganizationIdentityRefusal::Unprovisioned),
            Self::Provisioned { organization_id } if organization_id == requested_organization => {
                Ok(())
            }
            Self::Provisioned { .. } => Err(OrganizationIdentityRefusal::OrganizationMismatch),
        }
    }
}

#[cfg(test)]
#[path = "identity_test.rs"]
mod tests;
