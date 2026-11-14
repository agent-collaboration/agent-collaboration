//! Closed reasons a person request is refused; they carry no caller text.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum PersonRequestRefusal {
    #[error("DPoP authorization required")]
    DpopRequired,
    #[error("malformed compact JWT")]
    MalformedJwt,
    #[error("unsupported protected header")]
    UnsupportedHeader,
    #[error("issuer key not usable")]
    IssuerKey,
    #[error("invalid token signature")]
    TokenSignature,
    #[error("invalid proof signature")]
    ProofSignature,
    #[error("verification capability unavailable")]
    VerificationUnavailable,
    #[error("invalid person claims")]
    PersonClaims,
    #[error("wrong issuer")]
    Issuer,
    #[error("wrong audience")]
    Audience,
    #[error("wrong token profile")]
    Profile,
    #[error("invalid signed expiry bounds")]
    ExpiryBounds,
    #[error("effective expiry reached")]
    Expired,
    #[error("invalid proof key")]
    ProofKey,
    #[error("proof key does not match token binding")]
    KeyBinding,
    #[error("HTTP method does not match")]
    Method,
    #[error("external target does not match")]
    Target,
    #[error("proof timestamp outside window")]
    ProofTime,
    #[error("request timestamp outside window or at or below the watermark")]
    RequestTime,
    #[error("request ID must be the proof's UUIDv7 jti")]
    RequestId,
    #[error("access token hash does not match")]
    TokenHash,
    #[error("invalid operation body")]
    OperationBody,
    #[error("signed intent does not match the received operation")]
    Intent,
    #[error("request names another organization")]
    OrganizationMismatch,
    #[error("invalid trusted policy or clock")]
    Policy,
}
