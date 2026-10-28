use serde::{Deserialize, Serialize};
use std::future::Future;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AccessClientId(String);
impl AccessClientId {
    pub fn new(value: impl Into<String>) -> Result<Self, AccessRefusal> {
        let value = value.into();
        if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
            return Err(AccessRefusal::InvalidCaller);
        }
        Ok(Self(value))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for AccessClientId {
    type Error = AccessRefusal;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<AccessClientId> for String {
    fn from(value: AccessClientId) -> Self {
        value.0
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum AccessCaller {
    Machine { client_id: AccessClientId },
    Owner { email: String },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AccessRefusal {
    #[error("Access assertion missing")]
    MissingAssertion,
    #[error("Access assertion malformed")]
    MalformedAssertion,
    #[error("Access algorithm unsupported")]
    UnsupportedAlgorithm,
    #[error("Access issuer mismatch")]
    WrongIssuer,
    #[error("Access audience mismatch")]
    WrongAudience,
    #[error("Access assertion expired")]
    Expired,
    #[error("Access assertion not yet valid")]
    NotYetValid,
    #[error("Access caller invalid")]
    InvalidCaller,
    #[error("Access signing key unknown")]
    UnknownKey,
    #[error("Access signing key invalid")]
    InvalidKey,
    #[error("Access signature invalid")]
    BadSignature,
    #[error("Access key source unavailable")]
    KeySourceUnavailable,
    #[error("Access caller class refused")]
    WrongCallerClass,
    #[error("Access owner mismatch")]
    WrongOwner,
    #[error("Access route refused")]
    RouteRefused,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessPolicy {
    pub issuer: String,
    pub jwks_url: String,
    pub session_audience: String,
    pub admin_audience: String,
    pub owner_identity: String,
}
impl AccessPolicy {
    /// Cloudflare deployment configuration keeps its original validation and URLs.
    pub fn cloudflare(
        team_domain: String,
        session_audience: String,
        admin_audience: String,
        owner_identity: String,
    ) -> Result<Self, AccessRefusal> {
        if !team_domain.ends_with(".cloudflareaccess.com")
            || team_domain
                .bytes()
                .any(|c| !c.is_ascii_alphanumeric() && c != b'-' && c != b'.')
        {
            return Err(AccessRefusal::InvalidCaller);
        }
        let issuer = format!("https://{team_domain}");
        let policy = Self {
            jwks_url: format!("{issuer}/cdn-cgi/access/certs"),
            issuer,
            session_audience,
            admin_audience,
            owner_identity,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), AccessRefusal> {
        if !valid_key_url(&self.issuer)
            || !valid_key_url(&self.jwks_url)
            || self.session_audience.is_empty()
            || self.admin_audience.is_empty()
            || self.session_audience == self.admin_audience
            || self.owner_identity.trim().is_empty()
        {
            return Err(AccessRefusal::InvalidCaller);
        }
        Ok(())
    }
    #[must_use]
    pub fn issuer(&self) -> String {
        self.issuer.clone()
    }
    #[must_use]
    pub fn certs_url(&self) -> String {
        self.jwks_url.clone()
    }
}

fn valid_key_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && value.trim() == value
            && !value.chars().any(char::is_control)
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccessJwk {
    pub kid: String,
    pub kty: String,
    pub n: String,
    pub e: String,
    pub alg: Option<String>,
    #[serde(rename = "use")]
    pub key_use: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccessJwks {
    pub keys: Vec<AccessJwk>,
}
impl AccessJwks {
    pub fn key(&self, kid: &str) -> Result<&AccessJwk, AccessRefusal> {
        let mut matching = self.keys.iter().filter(|key| key.kid == kid);
        let key = matching.next().ok_or(AccessRefusal::UnknownKey)?;
        if matching.next().is_some()
            || key.kty != "RSA"
            || key.n.is_empty()
            || key.e.is_empty()
            || key.alg.as_ref().is_some_and(|alg| alg != "RS256")
            || key.key_use.as_ref().is_some_and(|usage| usage != "sig")
        {
            return Err(AccessRefusal::InvalidKey);
        }
        crate::access_key_profile::validate_access_key_profile(key)?;
        Ok(key)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureValidity {
    Valid,
    Invalid,
}
/// Only the shell consumes this future. Parsing, key selection, claim checks
/// and authorization remain synchronous core decisions.
pub trait AccessSignaturePort {
    fn verify<'a>(
        &'a self,
        key: &'a AccessJwk,
        assertion: &'a crate::ParsedAccessAssertion,
    ) -> impl Future<Output = Result<SignatureValidity, AccessRefusal>> + 'a;
}
