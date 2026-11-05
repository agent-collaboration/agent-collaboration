//! The private link asks the existing Fleet owner to commit provisioning.
use crate::identity_contract::{ProvisionOrganization, ProvisionOrganizationResult};
use std::future::Future;

pub trait OrganizationProvisioningPort: Send + Sync + 'static {
    /// Return Provisioned only after the stamp and its record have committed together.
    fn provision_organization(
        &self,
        request: &ProvisionOrganization,
    ) -> impl Future<Output = Result<ProvisionOrganizationResult, OrganizationProvisioningFailure>> + Send;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OrganizationProvisioningFailure {
    #[error("the request does not belong to this Fleet object")]
    OrganizationMismatch,
    #[error("organization provisioning is unavailable")]
    Unavailable,
    #[error("organization provisioning could not confirm its commit")]
    OutcomeUnknown,
}
