//! Fleet identity verification owns no storage, policy lifetime or host routing.
use super::fleet_statement_wire::{OrganizationStatementDigest, ParsedFleetStatement};
use crate::{
    Ed25519Verification, IssuerKeyCache, IssuerKeySource, IssuerSignatureRefusal,
    IssuerSignatureVerifier,
};
use fleet_identity_protocol::{OrganizationId, UnixSeconds};
use runtime_primitives::{Clock, SyncTime};
use std::sync::Arc;

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
        // RED checkpoint only: structural decoding and real crypto execute, but
        // semantic admission is deliberately incomplete for the first failing run.
        let _validity = self
            .signatures
            .verify(&parsed.header.kid, &parsed.signed, &parsed.signature)
            .await
            .map_err(FleetOrganizationStatementRefusal::VerificationUnavailable)?;
        let _ = (
            &self.trust.issuer,
            &self.trust.audience,
            &self.trust.organization,
            self.clock.now(),
            &parsed.header.typ,
            &parsed.header.alg,
            &parsed.claims.purpose,
            parsed.claims.version,
            &parsed.claims.iss,
            &parsed.claims.aud,
            &parsed.claims.sub,
        );
        Ok(VerifiedFleetOrganizationStatement {
            organization: parsed.claims.organization_id,
            key_id: parsed.header.kid,
            issued_at: UnixSeconds(parsed.claims.iat.0),
            expires_at: UnixSeconds(parsed.claims.exp.0),
            replaces: parsed.claims.replaces,
            digest: OrganizationStatementDigest::of_compact_jws(statement),
        })
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
        if now.unix_millis().div_euclid(1000) >= self.expires_at.0 {
            return Err(FleetOrganizationStatementRefusal::Expired);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FleetOrganizationStatementRefusal {
    #[error("Fleet organization statement is malformed")]
    Malformed,
    #[error("Fleet organization statement has the wrong token type")]
    WrongTokenType,
    #[error("Fleet organization statement algorithm is unsupported")]
    UnsupportedAlgorithm,
    #[error("Fleet organization statement signature is invalid")]
    BadSignature,
    #[error("Fleet organization statement has the wrong purpose")]
    WrongPurpose,
    #[error("Fleet organization statement version is unsupported")]
    UnsupportedVersion,
    #[error("Fleet organization statement has the wrong issuer")]
    WrongIssuer,
    #[error("Fleet organization statement has the wrong audience")]
    WrongAudience,
    #[error("Fleet organization statement names a different organization")]
    OrganizationMismatch,
    #[error("Fleet organization statement has expired")]
    Expired,
    #[error("Fleet organization statement is not yet valid")]
    NotYetValid,
    #[error("Fleet organization statement trust configuration is invalid")]
    InvalidConfiguration,
    #[error("Fleet organization statement verification is unavailable: {0}")]
    VerificationUnavailable(IssuerSignatureRefusal),
}

#[cfg(test)]
#[path = "fleet_statement_test.rs"]
pub(super) mod tests;
