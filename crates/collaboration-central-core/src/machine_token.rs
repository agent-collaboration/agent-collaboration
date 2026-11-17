//! Verify the issuer's machine access-token profile before live admission.
use crate::{
    Ed25519Verification, IssuerKeySource, IssuerSignatureVerifier, SignatureValidity,
    identity_contract::{ClientId, MachinePrincipal, MachineTokenClaims},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fleet_identity_protocol::{EnrollmentId, InstallationId, OrganizationId, UnixSeconds};
use runtime_primitives::Clock;
use serde::Deserialize;
use std::sync::Arc;

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Clone, Debug)]
pub struct MachineTokenPolicy {
    pub issuer: String,
    pub audience: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedMachineToken {
    organization_id: OrganizationId,
    installation_id: InstallationId,
    enrollment_id: EnrollmentId,
    client_id: ClientId,
    effective_expires_at: UnixSeconds,
}

impl VerifiedMachineToken {
    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        &self.organization_id
    }
    #[must_use]
    pub fn installation_id(&self) -> &InstallationId {
        &self.installation_id
    }
    #[must_use]
    pub fn enrollment_id(&self) -> &EnrollmentId {
        &self.enrollment_id
    }
    #[must_use]
    pub fn client_id(&self) -> &ClientId {
        &self.client_id
    }
    #[must_use]
    pub fn effective_expires_at(&self) -> UnixSeconds {
        self.effective_expires_at
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MachineTokenRefusal {
    #[error("machine token rejected")]
    Rejected,
    #[error("machine token verification unavailable")]
    Unavailable,
}

pub struct MachineTokenVerifier<Ed, Keys> {
    signatures: IssuerSignatureVerifier<Ed, Keys>,
    clock: Arc<dyn Clock>,
    policy: MachineTokenPolicy,
}

impl<Ed: Ed25519Verification, Keys: IssuerKeySource> MachineTokenVerifier<Ed, Keys> {
    #[must_use]
    pub fn new(
        signatures: IssuerSignatureVerifier<Ed, Keys>,
        clock: Arc<dyn Clock>,
        policy: MachineTokenPolicy,
    ) -> Self {
        Self {
            signatures,
            clock,
            policy,
        }
    }

    pub async fn verify(&self, token: &str) -> Result<VerifiedMachineToken, MachineTokenRefusal> {
        if self.policy.issuer.is_empty() || self.policy.audience.is_empty() {
            return Err(MachineTokenRefusal::Unavailable);
        }
        let parsed = CompactMachineToken::parse(token)?;
        let header: Header =
            serde_json::from_slice(&parsed.header).map_err(|_| MachineTokenRefusal::Rejected)?;
        if header.alg != "EdDSA" || header.typ != "at+jwt" || header.kid.is_empty() {
            return Err(MachineTokenRefusal::Rejected);
        }
        let claims: Claims =
            serde_json::from_slice(&parsed.claims).map_err(|_| MachineTokenRefusal::Rejected)?;
        if claims.iss != self.policy.issuer {
            return Err(MachineTokenRefusal::Rejected);
        }
        match self
            .signatures
            .verify(&header.kid, &parsed.signed, &parsed.signature)
            .await
        {
            Ok(SignatureValidity::Valid) => {}
            Ok(SignatureValidity::Invalid) => return Err(MachineTokenRefusal::Rejected),
            Err(crate::IssuerSignatureRefusal::Keys(
                crate::IssuerKeyResolutionRefusal::KeySet(_),
            )) => return Err(MachineTokenRefusal::Rejected),
            Err(_) => return Err(MachineTokenRefusal::Unavailable),
        }
        let now = self.clock.now().unix_millis().div_euclid(1000);
        claims.validate(&self.policy, now)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: String,
    typ: String,
    kid: String,
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    aud: String,
    iat: i64,
    exp: i64,
    client_id: String,
    azp: String,
    #[serde(flatten)]
    machine: MachineTokenClaims,
    #[serde(default, rename = "sid")]
    _sid: Option<()>,
    #[serde(default, rename = "workspace_id", deserialize_with = "reject_present")]
    _workspace_id: (),
}

fn reject_present<'de, D: serde::Deserializer<'de>>(_: D) -> Result<(), D::Error> {
    Err(serde::de::Error::custom("workspace_id is forbidden"))
}

impl Claims {
    fn validate(
        self,
        policy: &MachineTokenPolicy,
        now: i64,
    ) -> Result<VerifiedMachineToken, MachineTokenRefusal> {
        let rejected = MachineTokenRefusal::Rejected;
        let MachineTokenClaims {
            principal:
                MachinePrincipal::Machine {
                    installation_id,
                    enrollment_id,
                },
            organization_id,
            grant_expires_at: UnixSeconds(grant_expires_at),
        } = self.machine;
        if self.iss != policy.issuer
            || self.aud != policy.audience
            || self.client_id != self.azp
            || [self.iat, self.exp, grant_expires_at]
                .iter()
                .any(|value| !(0..=MAX_SAFE_INTEGER).contains(value))
            || self.exp <= self.iat
            || self.exp - self.iat > 3600
        {
            return Err(rejected);
        }
        let expiry = self
            .exp
            .checked_add(60)
            .filter(|value| *value <= MAX_SAFE_INTEGER)
            .ok_or(rejected)?;
        let future = now.checked_add(60).ok_or(rejected)?;
        if self.iat > future || now >= expiry || now >= grant_expires_at {
            return Err(rejected);
        }
        let effective_expires_at = expiry.min(grant_expires_at);
        effective_expires_at
            .checked_mul(1000)
            .filter(|millis| *millis <= MAX_SAFE_INTEGER)
            .ok_or(rejected)?;
        for id in [
            organization_id.as_str(),
            installation_id.as_str(),
            enrollment_id.as_str(),
            self.client_id.as_str(),
            self.azp.as_str(),
        ] {
            let uuid = durable_steps::Uuid::parse_str(id).map_err(|_| rejected)?;
            if !runtime_primitives::uuid_v7(uuid) || uuid.to_string() != id {
                return Err(rejected);
            }
        }
        Ok(VerifiedMachineToken {
            organization_id,
            installation_id,
            enrollment_id,
            client_id: self.client_id.try_into().map_err(|_| rejected)?,
            effective_expires_at: UnixSeconds(effective_expires_at),
        })
    }
}

struct CompactMachineToken {
    header: Vec<u8>,
    claims: Vec<u8>,
    signed: Vec<u8>,
    signature: Vec<u8>,
}
impl CompactMachineToken {
    fn parse(token: &str) -> Result<Self, MachineTokenRefusal> {
        let rejected = MachineTokenRefusal::Rejected;
        if token.len() > 16_384 {
            return Err(rejected);
        }
        let mut parts = token.split('.');
        let header = parts
            .next()
            .filter(|part| !part.is_empty())
            .ok_or(rejected)?;
        let claims = parts
            .next()
            .filter(|part| !part.is_empty())
            .ok_or(rejected)?;
        let signature = parts
            .next()
            .filter(|part| !part.is_empty())
            .ok_or(rejected)?;
        if parts.next().is_some() {
            return Err(rejected);
        }
        fn decode(part: &str) -> Result<Vec<u8>, MachineTokenRefusal> {
            let bytes = URL_SAFE_NO_PAD
                .decode(part)
                .map_err(|_| MachineTokenRefusal::Rejected)?;
            if URL_SAFE_NO_PAD.encode(&bytes) != part {
                return Err(MachineTokenRefusal::Rejected);
            }
            Ok(bytes)
        }
        Ok(Self {
            header: decode(header)?,
            claims: decode(claims)?,
            signed: format!("{header}.{claims}").into_bytes(),
            signature: decode(signature)?,
        })
    }
}

#[cfg(test)]
#[path = "machine_token_test.rs"]
mod tests;
