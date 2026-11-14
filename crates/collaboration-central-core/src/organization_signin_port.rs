//! The private sign-in link delegates organization work to a scoped runtime.
use crate::{
    identity_contract::{
        AuthorizationRevision, AuthorizationRevisionAcknowledgement,
        OrganizationStatementDigestReply, ProvisionOrganization, ProvisionOrganizationResult,
        ReadOrganizationStatementDigest,
    },
    organization_admin_contract::OrganizationAdminAnswer,
    organization_authority::OrganizationProvisioningFailure,
    person_request::PersonRequestInput,
};
use central_storage::StoreFailure;
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
}
impl<H: crate::ObjectHost, S: crate::WorkspaceStoreCalls> OrganizationSignInPort
    for crate::WorkspaceRuntime<H, S>
{
    async fn provision(
        &self,
        request: &ProvisionOrganization,
    ) -> Result<ProvisionOrganizationResult, OrganizationProvisioningFailure> {
        self.provision_organization(request).await
    }
    async fn digest(
        &self,
        request: &ReadOrganizationStatementDigest,
    ) -> Result<OrganizationStatementDigestReply, OrganizationProvisioningFailure> {
        self.read_statement_digest(request).await
    }
    async fn revision(
        &self,
        request: &AuthorizationRevision,
    ) -> Result<AuthorizationRevisionAcknowledgement, OrganizationProvisioningFailure> {
        self.deliver_authorization_revision(request).await
    }
    async fn admin(
        &self,
        organization: OrganizationId,
        input: PersonRequestInput,
    ) -> Result<OrganizationAdminAnswer, StoreFailure> {
        if organization != self.organization {
            return Ok(OrganizationAdminAnswer::Unauthenticated);
        }
        self.admin_person_request(&input).await
    }
}
