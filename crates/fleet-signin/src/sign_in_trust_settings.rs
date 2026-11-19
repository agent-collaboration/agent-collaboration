//! Deployment trust shared by the native and Worker sign-in adapters.
use crate::organization_authority::{
    FleetOrganizationStatementTrust, FleetOrganizationStatementVerifier, SignInVerification,
    SignInVerifiers,
};
use crate::person_request::{PersonRequestPolicy, PersonRequestVerifier};
use crate::{IssuerKeyCache, IssuerSignatureVerifier};
use fleet_contracts::FleetOrganizationStatementRefusal;
use fleet_identity_protocol::OrganizationId;
use fleet_runtime_ports::{Clock, Ed25519Verification, Es256Verification, IssuerKeySource};
use std::sync::Arc;

/// Hosts supply these settings explicitly when constructing an organization's
/// verifier. Transport addresses and the organization's clock belong to the host.
#[derive(Clone)]
pub struct SignInTrustSettings {
    pub issuer: String,
    /// Audience of organization stamps for this central deployment.
    pub audience: String,
    /// Audience of person tokens for central's admin surface.
    pub admin_audience: String,
    /// Central's external HTTPS origin, used for DPoP request targets.
    pub public_origin: String,
    pub jwks_url: String,
    /// The sign-in service's identity on records written for a person.
    /// Admin calls refuse when the deployment has not configured this identity.
    pub service_id: Option<message_board::ServiceId>,
}

impl SignInTrustSettings {
    /// Binds both verifiers to the organization's shared clock and key cache.
    pub fn for_organization<Ed, Es, Keys>(
        &self,
        organization: OrganizationId,
        clock: Arc<dyn Clock>,
        cache: &Arc<IssuerKeyCache>,
        keys: impl Fn() -> Keys,
        ed25519: impl Fn() -> Ed,
        es256: Es,
    ) -> Result<Arc<dyn SignInVerification>, FleetOrganizationStatementRefusal>
    where
        Ed: Ed25519Verification,
        Es: Es256Verification,
        Keys: IssuerKeySource + 'static,
    {
        let trust = FleetOrganizationStatementTrust::new(
            self.issuer.clone(),
            self.audience.clone(),
            organization,
        )?;
        Ok(Arc::new(SignInVerifiers {
            statements: FleetOrganizationStatementVerifier::new(
                ed25519(),
                keys(),
                cache.clone(),
                clock.clone(),
                trust,
            ),
            persons: PersonRequestVerifier::new(
                IssuerSignatureVerifier::new(ed25519(), keys(), cache.clone(), clock.clone()),
                es256,
                clock,
                PersonRequestPolicy {
                    issuer: self.issuer.clone(),
                    audience: self.admin_audience.clone(),
                    public_origin: self.public_origin.clone(),
                },
            ),
            sign_in_service: self.service_id.clone(),
        }))
    }
}
