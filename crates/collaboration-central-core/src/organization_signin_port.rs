//! The private sign-in link delegates organization work to a scoped runtime.
use crate::{
    identity_contract::{
        AuthorizationRevision, AuthorizationRevisionAcknowledgement,
        OrganizationStatementDigestReply, ProvisionOrganization, ProvisionOrganizationResult,
        ReadOrganizationStatementDigest,
    },
    organization_admin_contract::OrganizationAdminAnswer,
    person_request::PersonRequestInput,
};
use central_storage::StoreFailure;
use fleet_contracts::OrganizationProvisioningFailure;
use fleet_identity_protocol::OrganizationId;
use std::future::Future;

/// Hosts supply routing to the named organization's runtime; this port does
/// not grant trust or bypass that runtime's admission and commit boundary.
pub trait OrganizationSignInPort: Send + Sync + 'static {
    fn provision(
        &self,
        request: &ProvisionOrganization,
    ) -> impl Future<Output = Result<ProvisionOrganizationResult, OrganizationProvisioningFailure>> + Send;

    fn digest(
        &self,
        request: &ReadOrganizationStatementDigest,
    ) -> impl Future<
        Output = Result<OrganizationStatementDigestReply, OrganizationProvisioningFailure>,
    > + Send;

    fn revision(
        &self,
        request: &AuthorizationRevision,
    ) -> impl Future<
        Output = Result<AuthorizationRevisionAcknowledgement, OrganizationProvisioningFailure>,
    > + Send;

    fn admin(
        &self,
        organization: OrganizationId,
        input: PersonRequestInput,
    ) -> impl Future<Output = Result<OrganizationAdminAnswer, StoreFailure>> + Send;
    fn device_login(
        &self,
        request: &crate::identity_contract::DecideDeviceLogin,
    ) -> impl Future<Output = Result<crate::identity_contract::DeviceLoginDecision, StoreFailure>> + Send;
    fn machine_token(
        &self,
        request: &crate::identity_contract::DecideMachineToken,
    ) -> impl Future<Output = Result<crate::identity_contract::MachineTokenDecision, StoreFailure>> + Send;
}
impl<H: fleet_runtime_ports::CentralStore, S: fleet_runtime_ports::WorkspaceStores>
    OrganizationSignInPort for crate::WorkspaceRuntime<H, S>
{
    async fn provision(
        &self,
        request: &ProvisionOrganization,
    ) -> Result<ProvisionOrganizationResult, OrganizationProvisioningFailure> {
        let answer = self.provision_organization(request).await;
        self.after_signin_event().await;
        answer
    }
    async fn digest(
        &self,
        request: &ReadOrganizationStatementDigest,
    ) -> Result<OrganizationStatementDigestReply, OrganizationProvisioningFailure> {
        let answer = self.read_statement_digest(request).await;
        self.after_signin_event().await;
        answer
    }
    async fn revision(
        &self,
        request: &AuthorizationRevision,
    ) -> Result<AuthorizationRevisionAcknowledgement, OrganizationProvisioningFailure> {
        let answer = self.deliver_authorization_revision(request).await;
        self.after_signin_event().await;
        answer
    }
    async fn admin(
        &self,
        organization: OrganizationId,
        input: PersonRequestInput,
    ) -> Result<OrganizationAdminAnswer, StoreFailure> {
        if organization != self.organization {
            return Ok(OrganizationAdminAnswer::Unauthenticated);
        }
        let answer = self.admin_person_request(&input).await;
        self.after_signin_event().await;
        answer
    }
    async fn device_login(
        &self,
        request: &crate::identity_contract::DecideDeviceLogin,
    ) -> Result<crate::identity_contract::DeviceLoginDecision, StoreFailure> {
        let answer = self.decide_device_login(request).await;
        self.after_signin_event().await;
        answer
    }
    async fn machine_token(
        &self,
        request: &crate::identity_contract::DecideMachineToken,
    ) -> Result<crate::identity_contract::MachineTokenDecision, StoreFailure> {
        let answer = self.decide_machine_token(request).await;
        self.after_signin_event().await;
        answer
    }
}

impl<H: fleet_runtime_ports::CentralStore, S: fleet_runtime_ports::WorkspaceStores>
    crate::WorkspaceRuntime<H, S>
{
    /// Every sign-in entry is an event (P6): after its own work, and after its
    /// central turn is released, it runs the due batch and re-arms. An alarm
    /// problem never replaces the entry's answer; it is observed.
    async fn after_signin_event(&self) {
        let _ = self.after_event().await;
    }
}
