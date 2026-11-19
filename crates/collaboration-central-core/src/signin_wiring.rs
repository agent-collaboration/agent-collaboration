//! Composes the sign-in authority with the interim engine's alarm policy.
use crate::WorkspaceRuntime;
use crate::signin_alarm_policy::SignInAlarmPolicy;
use central_storage::StoreFailure;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use fleet_signin::{
    OrganizationSignIn,
    identity_contract::{
        AuthorizationRevision, AuthorizationRevisionAcknowledgement,
        OrganizationStatementDigestReply, ProvisionOrganization, ProvisionOrganizationResult,
        ReadOrganizationStatementDigest,
    },
    organization_authority::OrganizationProvisioningFailure,
};

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub(crate) fn sign_in(&self) -> OrganizationSignIn<'_, Host, Stores, SignInAlarmPolicy> {
        OrganizationSignIn::new(
            &self.fleet_runtime,
            &self.signin_authority,
            SignInAlarmPolicy,
        )
    }

    pub(crate) async fn prepare_signin_authority(&self) -> Result<(), StoreFailure> {
        self.sign_in().prepare_signin_authority().await
    }

    pub async fn provision_organization(
        &self,
        request: &ProvisionOrganization,
    ) -> Result<ProvisionOrganizationResult, OrganizationProvisioningFailure> {
        self.sign_in().provision_organization(request).await
    }

    pub(crate) async fn read_statement_digest(
        &self,
        request: &ReadOrganizationStatementDigest,
    ) -> Result<OrganizationStatementDigestReply, OrganizationProvisioningFailure> {
        self.sign_in().read_statement_digest(request).await
    }

    pub(crate) async fn deliver_authorization_revision(
        &self,
        revision: &AuthorizationRevision,
    ) -> Result<AuthorizationRevisionAcknowledgement, OrganizationProvisioningFailure> {
        self.sign_in()
            .deliver_authorization_revision(revision)
            .await
    }
}
