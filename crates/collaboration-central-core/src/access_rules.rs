use crate::{
    AccessCaller, AccessClientId, AccessPolicy, AccessRefusal, RegistryTime, SignatureValidity,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;

#[derive(Deserialize)]
struct AssertionHeader {
    alg: String,
    kid: String,
}
/// JWT `aud`: one string or an array of strings (RFC 7519 §4.1.3). Access
/// issues service-token assertions with a single string and identity
/// assertions with an array.
#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}
impl Audience {
    fn contains(&self, audience: &str) -> bool {
        match self {
            Self::One(one) => one == audience,
            Self::Many(many) => many.iter().any(|candidate| candidate == audience),
        }
    }
}
#[derive(Deserialize)]
struct AssertionClaims {
    iss: String,
    aud: Audience,
    exp: i64,
    /// Identity tokens carry `nbf`; Cloudflare's service-token tokens don't.
    #[serde(default)]
    nbf: Option<i64>,
    #[serde(rename = "type")]
    assertion_type: String,
    #[serde(default)]
    common_name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    sub: String,
}
pub struct ParsedAccessAssertion {
    kid: String,
    signed_bytes: Vec<u8>,
    signature: Vec<u8>,
    caller: AccessCaller,
}
impl ParsedAccessAssertion {
    pub fn parse(
        header: Option<&str>,
        policy: &AccessPolicy,
        path: &str,
        now: RegistryTime,
    ) -> Result<Self, AccessRefusal> {
        policy.validate()?;
        let token = header.ok_or(AccessRefusal::MissingAssertion)?;
        let mut parts = token.split('.');
        let head = parts.next().ok_or(AccessRefusal::MalformedAssertion)?;
        let body = parts.next().ok_or(AccessRefusal::MalformedAssertion)?;
        let tail = parts.next().ok_or(AccessRefusal::MalformedAssertion)?;
        if parts.next().is_some() {
            return Err(AccessRefusal::MalformedAssertion);
        }
        let decode = |encoded: &str| {
            URL_SAFE_NO_PAD
                .decode(encoded)
                .map_err(|_| AccessRefusal::MalformedAssertion)
        };
        let header: AssertionHeader = serde_json::from_slice(&decode(head)?)
            .map_err(|_| AccessRefusal::MalformedAssertion)?;
        if header.alg != "RS256" {
            return Err(AccessRefusal::UnsupportedAlgorithm);
        }
        if header.kid.is_empty() {
            return Err(AccessRefusal::UnknownKey);
        }
        let claims: AssertionClaims = serde_json::from_slice(&decode(body)?)
            .map_err(|_| AccessRefusal::MalformedAssertion)?;
        if claims.iss != policy.issuer() {
            return Err(AccessRefusal::WrongIssuer);
        }
        let audience = if path == "/v1/session" {
            &policy.session_audience
        } else if path.starts_with("/admin/v1/") {
            &policy.admin_audience
        } else {
            return Err(AccessRefusal::RouteRefused);
        };
        if !claims.aud.contains(audience) {
            return Err(AccessRefusal::WrongAudience);
        }
        if claims.exp <= now.0.timestamp() {
            return Err(AccessRefusal::Expired);
        }
        if claims.nbf.is_some_and(|nbf| nbf > now.0.timestamp()) {
            return Err(AccessRefusal::NotYetValid);
        }
        if claims.assertion_type != "app" {
            return Err(AccessRefusal::InvalidCaller);
        }
        let caller = match (claims.common_name, claims.email) {
            (Some(client_id), email)
                if claims.sub.is_empty() && email.as_ref().is_none_or(String::is_empty) =>
            {
                AccessCaller::Machine {
                    client_id: AccessClientId::new(client_id)?,
                }
            }
            (None, Some(email)) if !email.trim().is_empty() && !claims.sub.is_empty() => {
                AccessCaller::Owner { email }
            }
            _ => return Err(AccessRefusal::InvalidCaller),
        };
        Ok(Self {
            kid: header.kid,
            signed_bytes: format!("{head}.{body}").into_bytes(),
            signature: decode(tail)?,
            caller,
        })
    }
    #[must_use]
    pub fn kid(&self) -> &str {
        &self.kid
    }
    #[must_use]
    pub fn signed_bytes(&self) -> &[u8] {
        &self.signed_bytes
    }
    #[must_use]
    pub fn signature(&self) -> &[u8] {
        &self.signature
    }
    pub fn verified(self, validity: SignatureValidity) -> Result<AccessCaller, AccessRefusal> {
        match validity {
            SignatureValidity::Valid => Ok(self.caller),
            SignatureValidity::Invalid => Err(AccessRefusal::BadSignature),
        }
    }
}
pub fn authorize_access_route(
    policy: &AccessPolicy,
    path: &str,
    caller: &AccessCaller,
) -> Result<(), AccessRefusal> {
    match (path, caller) {
        ("/v1/session", AccessCaller::Machine { .. }) => Ok(()),
        (path, AccessCaller::Owner { email }) if path.starts_with("/admin/v1/") => {
            if email == &policy.owner_identity {
                Ok(())
            } else {
                Err(AccessRefusal::WrongOwner)
            }
        }
        (path, _) if path == "/v1/session" || path.starts_with("/admin/v1/") => {
            Err(AccessRefusal::WrongCallerClass)
        }
        _ => Err(AccessRefusal::RouteRefused),
    }
}
