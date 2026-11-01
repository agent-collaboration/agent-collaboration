//! Parse person proofs without treating parsed claims as authenticated callers.

use super::person_wire_values::PersonWireSeconds;
use crate::{SignatureValidity, identity_contract::PersonId};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fleet_identity_protocol::{OrganizationId, UnixMillis, UnixSeconds};
use message_board::RecordId;
use serde::Deserialize;

const SERVICE_CLOCK_SKEW_SECONDS: i64 = 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersonTokenAudience {
    Admin,
    Operator,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersonSignatureAlgorithm {
    EdDsa,
}

/// The issuer's UUIDv7 session identity, distinct from a Fleet harness session.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct SignInSessionRef(RecordId);

impl TryFrom<String> for SignInSessionRef {
    type Error = PersonTokenRefusal;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        RecordId::try_from(value)
            .map(Self)
            .map_err(|_| PersonTokenRefusal::NotAPerson)
    }
}

impl SignInSessionRef {
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Pinned configuration, never inferred from an untrusted token's claims.
pub struct PersonTokenPolicy {
    issuer: String,
    admin_audience: String,
    operator_audience: String,
    operator_client_id: String,
}

impl PersonTokenPolicy {
    pub fn new(
        issuer: String,
        admin_audience: String,
        operator_audience: String,
        operator_client_id: String,
    ) -> Result<Self, PersonTokenRefusal> {
        if !valid_resource(&issuer)
            || !valid_resource(&admin_audience)
            || !valid_resource(&operator_audience)
            || admin_audience == operator_audience
            || operator_client_id.is_empty()
        {
            return Err(PersonTokenRefusal::InvalidConfiguration);
        }
        Ok(Self {
            issuer,
            admin_audience,
            operator_audience,
            operator_client_id,
        })
    }

    fn audience(&self, purpose: PersonTokenAudience) -> &str {
        match purpose {
            PersonTokenAudience::Admin => &self.admin_audience,
            PersonTokenAudience::Operator => &self.operator_audience,
        }
    }
}

/// Borrowed input for F3a's verifier. No Debug implementation exposes the JWT.
pub struct PersonSignatureInput<'token> {
    key_id: &'token str,
    signed_bytes: &'token [u8],
    signature: &'token [u8],
}

impl PersonSignatureInput<'_> {
    #[must_use]
    pub const fn algorithm(&self) -> PersonSignatureAlgorithm {
        PersonSignatureAlgorithm::EdDsa
    }

    #[must_use]
    pub const fn key_id(&self) -> &str {
        self.key_id
    }

    #[must_use]
    pub const fn signed_bytes(&self) -> &[u8] {
        self.signed_bytes
    }

    #[must_use]
    pub const fn signature(&self) -> &[u8] {
        self.signature
    }
}

/// Parsing alone grants nothing. Verification and policy precede a live check.
pub struct ParsedPersonToken {
    bearer_token: String,
    key_id: String,
    signed_bytes: Vec<u8>,
    signature: Vec<u8>,
    claims: PersonClaims,
}

#[derive(Deserialize)]
struct TokenHeader {
    typ: String,
    alg: String,
    kid: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TokenAudience {
    One(String),
    Many(Vec<String>),
}

impl TokenAudience {
    fn matches(&self, expected: &str) -> bool {
        match self {
            Self::One(audience) => audience == expected,
            Self::Many(audiences) => {
                // Even a one-element array is not a person proof for this service.
                let _ = audiences;
                false
            }
        }
    }
}

// JWT claims deliberately permit registered extensions; HTTP bodies do not.
#[derive(Deserialize)]
struct PersonClaims {
    iss: String,
    sub: PersonId,
    aud: TokenAudience,
    exp: PersonWireSeconds,
    iat: PersonWireSeconds,
    scope: String,
    client_id: String,
    azp: String,
    sid: Option<String>,
    jti: String,
    organization_id: Option<OrganizationId>,
    second_factor_at: Option<PersonWireSeconds>,
    amr: Option<Vec<PersonAuthenticationMethod>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum PersonAuthenticationMethod {
    Otp,
    Hwk,
    Swk,
    User,
    Mfa,
}

impl ParsedPersonToken {
    pub fn parse(bearer_token: String) -> Result<Self, PersonTokenRefusal> {
        let mut segments = bearer_token.split('.');
        let header = segments.next().ok_or(PersonTokenRefusal::Malformed)?;
        let body = segments.next().ok_or(PersonTokenRefusal::Malformed)?;
        let signature = segments.next().ok_or(PersonTokenRefusal::Malformed)?;
        if segments.next().is_some() {
            return Err(PersonTokenRefusal::Malformed);
        }
        let parsed_header: TokenHeader = serde_json::from_slice(&decode_segment(header)?)
            .map_err(|_| PersonTokenRefusal::Malformed)?;
        if parsed_header.typ != "at+jwt" {
            return Err(PersonTokenRefusal::WrongTokenType);
        }
        if parsed_header.alg != "EdDSA" {
            return Err(PersonTokenRefusal::UnsupportedAlgorithm);
        }
        if parsed_header.kid.is_empty() {
            return Err(PersonTokenRefusal::Malformed);
        }
        let claims: PersonClaims = serde_json::from_slice(&decode_segment(body)?)
            .map_err(|_| PersonTokenRefusal::Malformed)?;
        let signed_bytes = format!("{header}.{body}").into_bytes();
        let signature = decode_segment(signature)?;
        if signature.is_empty() {
            return Err(PersonTokenRefusal::Malformed);
        }
        Ok(Self {
            bearer_token,
            key_id: parsed_header.kid,
            signed_bytes,
            signature,
            claims,
        })
    }

    #[must_use]
    pub fn signature_input(&self) -> PersonSignatureInput<'_> {
        PersonSignatureInput {
            key_id: &self.key_id,
            signed_bytes: &self.signed_bytes,
            signature: &self.signature,
        }
    }

    /// The trusted signature capability supplies validity for signature_input().
    pub fn verified(
        self,
        validity: SignatureValidity,
        policy: &PersonTokenPolicy,
        purpose: PersonTokenAudience,
        now: UnixMillis,
    ) -> Result<VerifiedPersonToken, PersonTokenRefusal> {
        if validity != SignatureValidity::Valid {
            return Err(PersonTokenRefusal::BadSignature);
        }
        let claims = self.claims;
        if claims.iss != policy.issuer {
            return Err(PersonTokenRefusal::WrongIssuer);
        }
        if !claims.aud.matches(policy.audience(purpose)) {
            return Err(PersonTokenRefusal::WrongAudience);
        }
        let now_seconds = now.0.div_euclid(1_000);
        let expires_at = claims.exp.into_unix_seconds();
        validate_token_expiry(expires_at, now)?;
        let latest_issue = now_seconds
            .checked_add(SERVICE_CLOCK_SKEW_SECONDS)
            .ok_or(PersonTokenRefusal::NotYetValid)?;
        if claims.iat.into_unix_seconds().0 > latest_issue {
            return Err(PersonTokenRefusal::NotYetValid);
        }
        if claims.client_id.is_empty() || claims.azp.is_empty() || claims.jti.is_empty() {
            return Err(PersonTokenRefusal::Malformed);
        }
        if purpose == PersonTokenAudience::Operator
            && (claims.client_id != policy.operator_client_id
                || claims.azp != policy.operator_client_id)
        {
            return Err(PersonTokenRefusal::WrongClient);
        }
        let session_ref =
            SignInSessionRef::try_from(claims.sid.ok_or(PersonTokenRefusal::NotAPerson)?)?;
        let organization_id = claims
            .organization_id
            .ok_or(PersonTokenRefusal::NoOrganization)?;
        let _ = claims.scope;
        let _ = claims.amr;
        Ok(VerifiedPersonToken {
            bearer_token: self.bearer_token,
            person_id: claims.sub,
            organization_id,
            session_ref,
            audience: policy.audience(purpose).to_owned(),
            expires_at,
            second_factor_at: claims
                .second_factor_at
                .map(PersonWireSeconds::into_unix_seconds),
            purpose,
        })
    }
}

/// A verified token still needs a fresh session confirmation and current owner.
/// It cannot be serialized into an allegedly authorized object call.
pub struct VerifiedPersonToken {
    bearer_token: String,
    person_id: PersonId,
    organization_id: OrganizationId,
    session_ref: SignInSessionRef,
    audience: String,
    expires_at: UnixSeconds,
    second_factor_at: Option<UnixSeconds>,
    purpose: PersonTokenAudience,
}

impl VerifiedPersonToken {
    pub(super) const fn purpose(&self) -> PersonTokenAudience {
        self.purpose
    }

    #[must_use]
    pub fn person_id(&self) -> &PersonId {
        &self.person_id
    }

    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        &self.organization_id
    }

    #[must_use]
    pub fn session_ref(&self) -> &SignInSessionRef {
        &self.session_ref
    }

    #[must_use]
    pub fn audience(&self) -> &str {
        &self.audience
    }

    #[must_use]
    pub const fn expires_at(&self) -> UnixSeconds {
        self.expires_at
    }

    #[must_use]
    pub const fn second_factor_at(&self) -> Option<UnixSeconds> {
        self.second_factor_at
    }

    /// Only the sign-in capability receives this secret, never audit or Debug.
    #[must_use]
    pub fn bearer_token(&self) -> &str {
        &self.bearer_token
    }
}

pub(super) fn validate_token_expiry(
    expires_at: UnixSeconds,
    now: UnixMillis,
) -> Result<(), PersonTokenRefusal> {
    let earliest_expiry = now
        .0
        .div_euclid(1_000)
        .checked_sub(SERVICE_CLOCK_SKEW_SECONDS)
        .ok_or(PersonTokenRefusal::Expired)?;
    if expires_at.0 <= earliest_expiry {
        return Err(PersonTokenRefusal::Expired);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PersonTokenRefusal {
    #[error("person token is malformed")]
    Malformed,
    #[error("person token has the wrong type")]
    WrongTokenType,
    #[error("person token algorithm is unsupported")]
    UnsupportedAlgorithm,
    #[error("person token signature is invalid")]
    BadSignature,
    #[error("person token issuer differs from configuration")]
    WrongIssuer,
    #[error("person token audience differs from this operation")]
    WrongAudience,
    #[error("person token was issued to another client")]
    WrongClient,
    #[error("person token has expired")]
    Expired,
    #[error("person token is not yet valid")]
    NotYetValid,
    #[error("token names no person session")]
    NotAPerson,
    #[error("person token names no organization")]
    NoOrganization,
    #[error("person token trust configuration is invalid")]
    InvalidConfiguration,
}

fn decode_segment(segment: &str) -> Result<Vec<u8>, PersonTokenRefusal> {
    URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| PersonTokenRefusal::Malformed)
}

fn valid_resource(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
    })
}
