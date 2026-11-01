//! Issuer key decoding and five-minute retention, separate from legacy Access.
use crate::RawJwks;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use message_board::IdentityValidationError;
use serde::Deserialize;
use std::{collections::BTreeSet, sync::Mutex};

const MAXIMUM_KEY_AGE_SECONDS: u64 = 300;

/// The provider owns key identifiers; they are opaque, nonempty strings.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Deserialize)]
#[serde(try_from = "String")]
pub struct IssuerKeyId(String);

impl TryFrom<String> for IssuerKeyId {
    type Error = IdentityValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(IdentityValidationError {
                field: "kid",
                requirement: "must identify the issuer's signing key",
            });
        }
        Ok(Self(value))
    }
}

impl IssuerKeyId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone)]
pub struct IssuerVerificationKey {
    key_id: IssuerKeyId,
    public_key: [u8; 32],
}

impl IssuerVerificationKey {
    #[must_use]
    pub fn key_id(&self) -> &IssuerKeyId {
        &self.key_id
    }

    #[must_use]
    pub const fn public_key(&self) -> &[u8; 32] {
        &self.public_key
    }
}

#[derive(Clone)]
pub struct IssuerKeySet {
    keys: Vec<IssuerVerificationKey>,
}

// This is a projection of the provider-owned public JWK envelope. Registered
// extensions may be present; every field used for verification is typed here.
#[derive(Deserialize)]
struct PublishedIssuerKeys {
    keys: Vec<PublishedIssuerKey>,
}

#[derive(Deserialize)]
struct PublishedIssuerKey {
    #[serde(rename = "kid")]
    key_id: IssuerKeyId,
    #[serde(rename = "kty")]
    _key_type: IssuerKeyType,
    #[serde(rename = "crv")]
    _curve: IssuerKeyCurve,
    #[serde(rename = "x")]
    public_key_base64: String,
    #[serde(rename = "alg")]
    _algorithm: Option<IssuerKeyAlgorithm>,
    #[serde(rename = "use")]
    _key_use: Option<IssuerKeyUse>,
}

#[derive(Deserialize)]
enum IssuerKeyType {
    #[serde(rename = "OKP")]
    OctetKeyPair,
}

#[derive(Deserialize)]
enum IssuerKeyCurve {
    Ed25519,
}

#[derive(Deserialize)]
enum IssuerKeyAlgorithm {
    #[serde(rename = "EdDSA")]
    EdDsa,
}

#[derive(Deserialize)]
enum IssuerKeyUse {
    #[serde(rename = "sig")]
    Signature,
}

impl IssuerKeySet {
    pub fn decode(bytes: &[u8]) -> Result<Self, IssuerKeyRefusal> {
        let published: PublishedIssuerKeys =
            serde_json::from_slice(bytes).map_err(|_| IssuerKeyRefusal::MalformedKeySet)?;
        let mut identities = BTreeSet::new();
        let mut keys = Vec::with_capacity(published.keys.len());
        for published in published.keys {
            if !identities.insert(published.key_id.clone()) {
                return Err(IssuerKeyRefusal::MalformedKeySet);
            }
            let public_key = URL_SAFE_NO_PAD
                .decode(published.public_key_base64)
                .map_err(|_| IssuerKeyRefusal::MalformedKeySet)?
                .try_into()
                .map_err(|_| IssuerKeyRefusal::MalformedKeySet)?;
            keys.push(IssuerVerificationKey {
                key_id: published.key_id,
                public_key,
            });
        }
        Ok(Self { keys })
    }

    pub fn key(&self, key_id: &IssuerKeyId) -> Result<&IssuerVerificationKey, IssuerKeyRefusal> {
        self.keys
            .iter()
            .find(|key| &key.key_id == key_id)
            .ok_or(IssuerKeyRefusal::UnknownKey)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IssuerKeyRefusal {
    #[error("the issuer's public key set is malformed")]
    MalformedKeySet,
    #[error("the issuer's signing key is unknown")]
    UnknownKey,
    #[error("the issuer key cache is unavailable")]
    CacheUnavailable,
}

#[derive(Default)]
pub struct IssuerKeyCache {
    cached: Mutex<Option<CachedIssuerKeys>>,
}

struct CachedIssuerKeys {
    source: String,
    keys: IssuerKeySet,
    expires_at: DateTime<Utc>,
}

impl IssuerKeyCache {
    pub fn fresh(
        &self,
        configured_source: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<IssuerKeySet>, IssuerKeyRefusal> {
        let cached = self
            .cached
            .lock()
            .map_err(|_| IssuerKeyRefusal::CacheUnavailable)?;
        Ok(cached
            .as_ref()
            .filter(|entry| entry.source == configured_source && now < entry.expires_at)
            .map(|entry| entry.keys.clone()))
    }

    pub fn remember(
        &self,
        configured_source: String,
        response: RawJwks,
        received_at: DateTime<Utc>,
    ) -> Result<IssuerKeySet, IssuerKeyRefusal> {
        let keys = IssuerKeySet::decode(&response.body)?;
        let max_age = response
            .cache_control
            .as_deref()
            .and_then(|header| {
                header
                    .split(',')
                    .find_map(|part| part.trim().strip_prefix("max-age=")?.parse::<u64>().ok())
            })
            .unwrap_or(MAXIMUM_KEY_AGE_SECONDS)
            .min(MAXIMUM_KEY_AGE_SECONDS);
        let expires_at = received_at
            .checked_add_signed(chrono::TimeDelta::seconds(
                i64::try_from(max_age).map_err(|_| IssuerKeyRefusal::CacheUnavailable)?,
            ))
            .ok_or(IssuerKeyRefusal::CacheUnavailable)?;
        *self
            .cached
            .lock()
            .map_err(|_| IssuerKeyRefusal::CacheUnavailable)? = Some(CachedIssuerKeys {
            source: configured_source,
            keys: keys.clone(),
            expires_at,
        });
        Ok(keys)
    }
}
