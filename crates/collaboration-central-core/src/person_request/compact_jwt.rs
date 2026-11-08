//! Compact JWS parsing shared by the person token and its DPoP proof.
use super::refusal::PersonRequestRefusal;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use durable_steps::Uuid;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

pub(crate) const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

pub(crate) struct CompactJwt {
    pub header: Vec<u8>,
    pub claims: Vec<u8>,
    pub signed: Vec<u8>,
    pub signature: Vec<u8>,
}

impl CompactJwt {
    pub fn parse(token: &str) -> Result<Self, PersonRequestRefusal> {
        // Bound parse work before decoding caller-controlled data.
        if token.len() > 16_384 {
            return Err(PersonRequestRefusal::MalformedJwt);
        }
        let mut parts = token.split('.');
        let header = parts.next().ok_or(PersonRequestRefusal::MalformedJwt)?;
        let claims = parts.next().ok_or(PersonRequestRefusal::MalformedJwt)?;
        let signature = parts.next().ok_or(PersonRequestRefusal::MalformedJwt)?;
        if parts.next().is_some() || header.is_empty() || claims.is_empty() || signature.is_empty()
        {
            return Err(PersonRequestRefusal::MalformedJwt);
        }
        Ok(Self {
            header: decode(header, PersonRequestRefusal::MalformedJwt)?,
            claims: decode(claims, PersonRequestRefusal::MalformedJwt)?,
            signed: format!("{header}.{claims}").into_bytes(),
            signature: decode(signature, PersonRequestRefusal::MalformedJwt)?,
        })
    }
}

/// Strict base64url: the encoding must round-trip exactly.
pub(crate) fn decode(
    encoded: &str,
    refusal: PersonRequestRefusal,
) -> Result<Vec<u8>, PersonRequestRefusal> {
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| refusal)?;
    if URL_SAFE_NO_PAD.encode(&bytes) != encoded {
        return Err(refusal);
    }
    Ok(bytes)
}

pub(crate) fn from_json<Value: DeserializeOwned>(
    bytes: &[u8],
    refusal: PersonRequestRefusal,
) -> Result<Value, PersonRequestRefusal> {
    serde_json::from_slice(bytes).map_err(|_| refusal)
}

pub(crate) fn sha256_url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(bytes))
}

pub(crate) fn uuid_v7(
    value: &str,
    refusal: PersonRequestRefusal,
) -> Result<Uuid, PersonRequestRefusal> {
    let id = Uuid::parse_str(value).map_err(|_| refusal)?;
    if !runtime_primitives::uuid_v7(id) || id.to_string() != value {
        return Err(refusal);
    }
    Ok(id)
}

/// A UUIDv7's embedded Unix-millisecond timestamp.
pub(crate) fn uuid_v7_millis(id: Uuid) -> Option<i64> {
    let (seconds, nanos) = id.get_timestamp()?.to_unix();
    let millis = seconds
        .checked_mul(1000)?
        .checked_add(u64::from(nanos / 1_000_000))?;
    i64::try_from(millis).ok()
}
