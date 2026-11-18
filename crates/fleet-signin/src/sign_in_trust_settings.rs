//! Deployment trust shared by the native and Worker sign-in adapters.

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
