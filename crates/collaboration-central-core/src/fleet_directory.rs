//! Shared directory logic reaches an organization's central through this port.
use crate::{
    CallOutcome,
    identity_contract::{InitializeOrganization, OrganizationInitialization, ResourceUri},
};
use std::future::Future;

/// F3a supplies service authentication and network transport at each host.
/// An unreceived or unreadable reply remains Failed, so the durable sender
/// retains its creating state and retries the same command.
pub trait OrganizationInitializationPort: Send + Sync + 'static {
    fn initialize_organization(
        &self,
        destination: ResourceUri,
        request: InitializeOrganization,
    ) -> impl Future<Output = CallOutcome<OrganizationInitialization>> + Send;
}
