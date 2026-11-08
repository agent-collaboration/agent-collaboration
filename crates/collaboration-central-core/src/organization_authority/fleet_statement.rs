//! Fleet identity verification owns no storage, policy lifetime or host routing.
use super::fleet_statement_refusal::FleetOrganizationStatementRefusal;
use super::fleet_statement_wire::{OrganizationStatementDigest, ParsedFleetStatement};
use crate::{
    Ed25519Verification, IssuerKeyCache, IssuerKeySource, IssuerSignatureVerifier,
    SignatureValidity,
};
use fleet_identity_protocol::{OrganizationId, UnixSeconds};
use runtime_primitives::{Clock, SyncTime};
use std::sync::Arc;

/// Owner choice 3f: a statement lives 120 days and is renewed with 60 days left.
/// The sign-in service owns issuance; central refuses anything longer-lived.
pub const MAXIMUM_STATEMENT_LIFETIME_SECONDS: i64 = 10_368_000;

pub struct FleetOrganizationStatementTrust {
    issuer: String,
    audience: String,
    organization: OrganizationId,
}

impl FleetOrganizationStatementTrust {
    pub fn new(
        issuer: String,
        audience: String,
        organization: OrganizationId,
    ) -> Result<Self, FleetOrganizationStatementRefusal> {
        if issuer.is_empty()
            || audience.is_empty()
            || issuer.contains('\0')
            || audience.contains('\0')
        {
            return Err(FleetOrganizationStatementRefusal::InvalidConfiguration);
        }
        Ok(Self {
            issuer,
            audience,
            organization,
        })
    }
}

pub struct FleetOrganizationStatementVerifier<Crypto, Keys> {
    signatures: IssuerSignatureVerifier<Crypto, Keys>,
    clock: Arc<dyn Clock>,
    trust: FleetOrganizationStatementTrust,
}

impl<Crypto: Ed25519Verification, Keys: IssuerKeySource>
    FleetOrganizationStatementVerifier<Crypto, Keys>
{
    #[must_use]
    pub fn new(
        crypto: Crypto,
        keys: Keys,
        cache: Arc<IssuerKeyCache>,
        clock: Arc<dyn Clock>,
        trust: FleetOrganizationStatementTrust,
    ) -> Self {
        Self {
            signatures: IssuerSignatureVerifier::new(crypto, keys, cache, clock.clone()),
            clock,
            trust,
        }
    }

    pub async fn verify(
        &self,
        statement: &str,
    ) -> Result<VerifiedFleetOrganizationStatement, FleetOrganizationStatementRefusal> {
        let parsed = ParsedFleetStatement::parse(statement)?;
        if parsed.header.typ != "fleet-organization-stamp+jwt" {
            return Err(FleetOrganizationStatementRefusal::WrongTokenType);
        }
        if parsed.header.alg != "EdDSA" {
            return Err(FleetOrganizationStatementRefusal::UnsupportedAlgorithm);
        }
        if parsed.header.kid.is_empty() || parsed.signature.len() != 64 {
            return Err(FleetOrganizationStatementRefusal::Malformed);
        }
        let validity = self
            .signatures
            .verify(&parsed.header.kid, &parsed.signed, &parsed.signature)
            .await
            .map_err(FleetOrganizationStatementRefusal::VerificationUnavailable)?;
        if validity != SignatureValidity::Valid {
            return Err(FleetOrganizationStatementRefusal::BadSignature);
        }
        if parsed.claims.purpose != "fleet-organization-stamp" {
            return Err(FleetOrganizationStatementRefusal::WrongPurpose);
        }
        if parsed.claims.version != 1 {
            return Err(FleetOrganizationStatementRefusal::UnsupportedVersion);
        }
        if parsed.claims.iss != self.trust.issuer {
            return Err(FleetOrganizationStatementRefusal::WrongIssuer);
        }
        if parsed.claims.aud != self.trust.audience {
            return Err(FleetOrganizationStatementRefusal::WrongAudience);
        }
        if parsed.claims.sub != parsed.claims.organization_id
            || parsed.claims.organization_id != self.trust.organization
        {
            return Err(FleetOrganizationStatementRefusal::OrganizationMismatch);
        }
        if parsed.claims.exp.0 <= parsed.claims.iat.0 {
            return Err(FleetOrganizationStatementRefusal::Malformed);
        }
        if parsed.claims.exp.0 - parsed.claims.iat.0 > MAXIMUM_STATEMENT_LIFETIME_SECONDS {
            return Err(FleetOrganizationStatementRefusal::LifetimeExceedsPolicy);
        }
        let verified = VerifiedFleetOrganizationStatement {
            organization: parsed.claims.organization_id,
            key_id: parsed.header.kid,
            issued_at: UnixSeconds(parsed.claims.iat.0),
            expires_at: UnixSeconds(parsed.claims.exp.0),
            replaces: parsed.claims.replaces,
            digest: OrganizationStatementDigest::of_compact_jws(statement),
        };
        // Sample after key fetch/crypto, so verification cannot outlive its exp.
        verified.require_valid_at(self.clock.now())?;
        Ok(verified)
    }
}

/// Constructed only by the signature/profile verifier; not a person permission.
#[derive(Debug)]
pub struct VerifiedFleetOrganizationStatement {
    organization: OrganizationId,
    key_id: String,
    issued_at: UnixSeconds,
    expires_at: UnixSeconds,
    replaces: Option<OrganizationStatementDigest>,
    digest: OrganizationStatementDigest,
}

impl VerifiedFleetOrganizationStatement {
    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        &self.organization
    }
    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
    #[must_use]
    pub const fn issued_at(&self) -> UnixSeconds {
        self.issued_at
    }
    #[must_use]
    pub const fn expires_at(&self) -> UnixSeconds {
        self.expires_at
    }
    #[must_use]
    pub fn replaces(&self) -> Option<&OrganizationStatementDigest> {
        self.replaces.as_ref()
    }
    #[must_use]
    pub fn digest(&self) -> &OrganizationStatementDigest {
        &self.digest
    }

    pub fn require_valid_at(&self, now: SyncTime) -> Result<(), FleetOrganizationStatementRefusal> {
        let now_seconds = now.unix_millis().div_euclid(1000);
        if now_seconds < self.issued_at.0 {
            return Err(FleetOrganizationStatementRefusal::NotYetValid);
        }
        if now_seconds >= self.expires_at.0 {
            return Err(FleetOrganizationStatementRefusal::Expired);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "fleet_statement_test.rs"]
mod tests;
