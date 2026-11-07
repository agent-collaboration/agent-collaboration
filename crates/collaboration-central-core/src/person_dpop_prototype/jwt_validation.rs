use super::request_contract::{DpopRefusal, Ed25519Jwk, PersonDpopPolicy};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use durable_steps::Uuid;
use serde::{Deserialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

pub(super) const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

pub(super) struct CompactJwt {
    pub header: Vec<u8>,
    pub claims: Vec<u8>,
    pub signed: Vec<u8>,
    pub signature: Vec<u8>,
}
impl CompactJwt {
    pub fn parse(token: &str) -> Result<Self, DpopRefusal> {
        // Bound allocation/parse work before decoding attacker-controlled data.
        if token.len() > 16_384 {
            return Err(DpopRefusal::MalformedJwt);
        }
        let mut parts = token.split('.');
        let header = parts.next().ok_or(DpopRefusal::MalformedJwt)?;
        let claims = parts.next().ok_or(DpopRefusal::MalformedJwt)?;
        let signature = parts.next().ok_or(DpopRefusal::MalformedJwt)?;
        if parts.next().is_some() || header.is_empty() || claims.is_empty() || signature.is_empty()
        {
            return Err(DpopRefusal::MalformedJwt);
        }
        Ok(Self {
            header: decode(header, DpopRefusal::MalformedJwt)?,
            claims: decode(claims, DpopRefusal::MalformedJwt)?,
            signed: format!("{header}.{claims}").into_bytes(),
            signature: decode(signature, DpopRefusal::MalformedJwt)?,
        })
    }
}

pub(super) fn decode(encoded: &str, refusal: DpopRefusal) -> Result<Vec<u8>, DpopRefusal> {
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| refusal)?;
    if URL_SAFE_NO_PAD.encode(&bytes) != encoded {
        return Err(refusal);
    }
    Ok(bytes)
}

pub(super) fn from_json<Value: DeserializeOwned>(
    bytes: &[u8],
    refusal: DpopRefusal,
) -> Result<Value, DpopRefusal> {
    serde_json::from_slice(bytes).map_err(|_| refusal)
}

pub(super) fn sha256_url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(bytes))
}

pub(super) fn uuid_v7(value: &str, refusal: DpopRefusal) -> Result<Uuid, DpopRefusal> {
    let id = Uuid::parse_str(value).map_err(|_| refusal)?;
    if !runtime_primitives::uuid_v7(id) || id.to_string() != value {
        return Err(refusal);
    }
    Ok(id)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TokenHeader {
    pub alg: String,
    pub kid: String,
    pub typ: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct IssuerJwks {
    pub keys: Vec<Ed25519Jwk>,
}
impl IssuerJwks {
    pub fn select(&self, kid: &str) -> Result<&Ed25519Jwk, DpopRefusal> {
        let mut candidates = self.keys.iter().filter(|key| key.kid == kid);
        let key = candidates.next().ok_or(DpopRefusal::IssuerKey)?;
        if kid.is_empty()
            || candidates.next().is_some()
            || key.kty != "OKP"
            || key.crv != "Ed25519"
            || key.alg != "EdDSA"
            || key.key_use.as_ref().is_some_and(|usage| usage != "sig")
            || decode(&key.x, DpopRefusal::IssuerKey)?.len() != 32
        {
            return Err(DpopRefusal::IssuerKey);
        }
        Ok(key)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct Confirmation {
    jkt: String,
}

#[derive(Deserialize)]
pub(super) struct PersonClaims {
    iss: String,
    aud: Audience,
    pub sub: String,
    pub sid: String,
    pub organization_id: String,
    token_profile: String,
    iat: i64,
    exp: i64,
    cnf: Confirmation,
    session_expires_at_ms: i64,
    authorization_snapshot_at_ms: i64,
    authorization_snapshot_valid_until_ms: i64,
}
impl PersonClaims {
    pub fn key_thumbprint(&self) -> &str {
        &self.cnf.jkt
    }
    pub fn validate(&self, policy: &PersonDpopPolicy, now_ms: i64) -> Result<i64, DpopRefusal> {
        if self.iss != policy.issuer {
            return Err(DpopRefusal::Issuer);
        }
        let audience_matches = match &self.aud {
            Audience::One(one) => one == &policy.audience,
            Audience::Many(many) => many.iter().any(|one| one == &policy.audience),
        };
        if !audience_matches {
            return Err(DpopRefusal::Audience);
        }
        if self.token_profile != "person-fleet" {
            return Err(DpopRefusal::Profile);
        }
        for id in [&self.sub, &self.sid, &self.organization_id] {
            uuid_v7(id, DpopRefusal::PersonClaims)?;
        }
        if decode(&self.cnf.jkt, DpopRefusal::PersonClaims)?.len() != 32
            || self.iat < 0
            || self.iat > now_ms / 1000
            || self.exp <= self.iat
        {
            return Err(DpopRefusal::PersonClaims);
        }
        let jwt_expiry_ms = self
            .exp
            .checked_mul(1000)
            .filter(|value| *value <= MAX_SAFE_INTEGER)
            .ok_or(DpopRefusal::ExpiryBounds)?;
        let snapshot_ms = self.authorization_snapshot_at_ms;
        let snapshot_deadline_ms = self.authorization_snapshot_valid_until_ms;
        let session_deadline_ms = self.session_expires_at_ms;
        if snapshot_ms < 0
            || snapshot_ms > now_ms
            || snapshot_deadline_ms <= snapshot_ms
            || snapshot_deadline_ms > MAX_SAFE_INTEGER
            || snapshot_deadline_ms - snapshot_ms > 300_000
            || session_deadline_ms <= snapshot_ms
            || session_deadline_ms > MAX_SAFE_INTEGER
        {
            return Err(DpopRefusal::ExpiryBounds);
        }
        let effective_expiry_ms = jwt_expiry_ms
            .min(snapshot_deadline_ms)
            .min(session_deadline_ms);
        if now_ms >= effective_expiry_ms {
            return Err(DpopRefusal::Expired);
        }
        Ok(effective_expiry_ms)
    }
}
