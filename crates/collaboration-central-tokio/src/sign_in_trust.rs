//! Native checks of the sign-in service's statements: ring Ed25519 over the
//! keys it publishes, fetched with reqwest and cached by the shared core cache.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    Ed25519Verification, IssuerKeyCache, IssuerKeySource, IssuerSignatureVerifier, KeyFetchFailure,
    RawJwks, SignatureValidity, VerifyFailure,
    organization_authority::{
        FleetOrganizationStatementRefusal, FleetOrganizationStatementTrust,
        FleetOrganizationStatementVerifier, SignInVerification, SignInVerifiers,
        VerifiedFleetOrganizationStatement,
    },
    person_request::{
        Es256Verification, PersonRequestInput, PersonRequestPolicy, PersonRequestRefusal,
        PersonRequestVerifier, ProofJwk, VerifiedPersonRequest,
    },
};
use fleet_identity_protocol::OrganizationId;
use ring::signature::{ECDSA_P256_SHA256_FIXED, ED25519, UnparsedPublicKey};
use runtime_primitives::Clock;
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

/// The deployment's sign-in issuer, the audience its stamps name for this
/// central, the audience its person tokens name for central's admin surface,
/// central's external origin (the DPoP `htu` base), where the issuer
/// publishes its keys, and the private link's socket path.
#[derive(Clone, Debug)]
pub struct SignInLinkSettings {
    pub issuer: String,
    pub audience: String,
    pub admin_audience: String,
    pub public_origin: String,
    pub jwks_url: String,
    pub socket: PathBuf,
}

/// One per process: every Fleet object shares the key cache and HTTP client.
pub(crate) struct NativeSignInTrust {
    settings: SignInLinkSettings,
    client: reqwest::Client,
    cache: Arc<IssuerKeyCache>,
}

impl NativeSignInTrust {
    pub(crate) fn new(settings: SignInLinkSettings) -> Result<Self, reqwest::Error> {
        Ok(Self {
            settings,
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
            cache: Arc::default(),
        })
    }

    /// A check bound to one organization's Fleet object and its clock.
    pub(crate) fn statement_check(
        &self,
        organization: OrganizationId,
        clock: Arc<dyn Clock>,
    ) -> Option<Arc<dyn SignInVerification>> {
        let trust = FleetOrganizationStatementTrust::new(
            self.settings.issuer.clone(),
            self.settings.audience.clone(),
            organization,
        )
        .ok()?;
        let keys = || SignInKeys {
            client: self.client.clone(),
            source: self.settings.jwks_url.clone(),
        };
        Some(Arc::new(SignInVerifiers {
            statements: FleetOrganizationStatementVerifier::new(
                NativeEd25519,
                keys(),
                self.cache.clone(),
                clock.clone(),
                trust,
            ),
            persons: PersonRequestVerifier::new(
                IssuerSignatureVerifier::new(
                    NativeEd25519,
                    keys(),
                    self.cache.clone(),
                    clock.clone(),
                ),
                NativeEs256,
                clock,
                PersonRequestPolicy {
                    issuer: self.settings.issuer.clone(),
                    audience: self.settings.admin_audience.clone(),
                    public_origin: self.settings.public_origin.clone(),
                },
            ),
        }))
    }
}

#[derive(Clone)]
struct NativeEs256;
impl Es256Verification for NativeEs256 {
    async fn verify_es256(
        &self,
        key: &ProofJwk,
        signed: &[u8],
        signature: &[u8],
    ) -> Result<SignatureValidity, VerifyFailure> {
        let decode = |component: &str| {
            URL_SAFE_NO_PAD
                .decode(component)
                .map_err(|_| VerifyFailure::UnsupportedKey)
        };
        // SEC1 uncompressed point: 04 || X || Y.
        let mut public_key = vec![4];
        public_key.extend(decode(&key.x)?);
        public_key.extend(decode(&key.y)?);
        Ok(
            if UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, public_key)
                .verify(signed, signature)
                .is_ok()
            {
                SignatureValidity::Valid
            } else {
                SignatureValidity::Invalid
            },
        )
    }
}

#[derive(Clone)]
struct NativeEd25519;
impl Ed25519Verification for NativeEd25519 {
    async fn verify_ed25519(
        &self,
        public_key: &[u8],
        signed: &[u8],
        signature: &[u8],
    ) -> Result<SignatureValidity, VerifyFailure> {
        Ok(
            if UnparsedPublicKey::new(&ED25519, public_key)
                .verify(signed, signature)
                .is_ok()
            {
                SignatureValidity::Valid
            } else {
                SignatureValidity::Invalid
            },
        )
    }
}

struct SignInKeys {
    client: reqwest::Client,
    source: String,
}
impl IssuerKeySource for SignInKeys {
    fn configured_source(&self) -> &str {
        &self.source
    }
    async fn fetch_issuer_keys(&self) -> Result<RawJwks, KeyFetchFailure> {
        let response = self
            .client
            .get(&self.source)
            .send()
            .await
            .map_err(|_| KeyFetchFailure::Unreachable)?;
        let status = response.status();
        let response = response
            .error_for_status()
            .map_err(|_| KeyFetchFailure::BadResponse {
                status: status.as_u16(),
            })?;
        let cache_control = response
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response
            .bytes()
            .await
            .map_err(|_| KeyFetchFailure::Unreachable)?
            .to_vec();
        Ok(RawJwks {
            body,
            cache_control,
        })
    }
}

/// Every native Fleet object gets this check when it is built, including the
/// stores found on disk at startup; it reads the process's sign-in trust when
/// a statement is checked, so installing the link later still applies to them.
pub(crate) struct DeferredSignInCheck {
    pub(crate) trust: Arc<OnceLock<Arc<NativeSignInTrust>>>,
    pub(crate) organization: OrganizationId,
    pub(crate) clock: Arc<dyn Clock>,
}
impl SignInVerification for DeferredSignInCheck {
    fn verify_statement<'a>(
        &'a self,
        statement: &'a str,
    ) -> durable_steps::BoxFuture<
        'a,
        Result<VerifiedFleetOrganizationStatement, FleetOrganizationStatementRefusal>,
    > {
        Box::pin(async move {
            let check = self
                .trust
                .get()
                .and_then(|trust| {
                    trust.statement_check(self.organization.clone(), self.clock.clone())
                })
                .ok_or(FleetOrganizationStatementRefusal::InvalidConfiguration)?;
            check.verify_statement(statement).await
        })
    }
    fn verify_person_request<'a>(
        &'a self,
        input: &'a PersonRequestInput,
    ) -> durable_steps::BoxFuture<'a, Result<VerifiedPersonRequest, PersonRequestRefusal>> {
        Box::pin(async move {
            let check = self
                .trust
                .get()
                .and_then(|trust| {
                    trust.statement_check(self.organization.clone(), self.clock.clone())
                })
                .ok_or(PersonRequestRefusal::Policy)?;
            check.verify_person_request(input).await
        })
    }
}
