use crate::{SignatureValidity, VerifyFailure};
use serde::{Deserialize, Serialize};
use std::future::Future;

/// Only signature mechanics vary by host; claims and hashes belong to the core.
/// Futures intentionally have no Send requirement: WebCrypto is isolate-local.
pub trait DpopCrypto {
    fn verify_issuer<'a>(
        &'a self,
        key: &'a Ed25519Jwk,
        signed: &'a [u8],
        signature: &'a [u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + 'a;
    fn verify_proof<'a>(
        &'a self,
        key: &'a Es256Jwk,
        signed: &'a [u8],
        signature: &'a [u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + 'a;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ed25519Jwk {
    pub kid: String,
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub alg: String,
    #[serde(rename = "use")]
    pub key_use: String,
}

/// Public proof key only. A private `d`, `crit`, or alternate key source is refused.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Es256Jwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PersonDpopPolicy {
    pub issuer: String,
    pub audience: String,
    /// Pinned external endpoint; never derived from forwarding headers or the proof.
    pub public_htu: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PersonDpopInput {
    pub authorization: String,
    pub proof: String,
    pub request_id: String,
    pub method: String,
    /// Actual received body, parsed once as the experiment's closed operation.
    pub body: String,
    /// A supplied durable watermark is checked but never stored/advanced here.
    pub reject_through_ms: i64,
}

/// Specimen only: this is not the Fleet wire contract. Every semantic field is hashed.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "camelCase")]
pub enum PrototypeOperation {
    RenameWorkspace {
        organization_id: String,
        operation_id: String,
        workspace_id: String,
        display_name: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DpopVerification {
    Verified { request: VerifiedPersonRequest },
    Refused { reason: DpopRefusal },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct VerifiedPersonRequest {
    pub person_id: String,
    pub session_id: String,
    pub organization_id: String,
    pub request_id: String,
    pub operation_id: String,
    pub proof_key_thumbprint: String,
    pub intent_digest: String,
    pub effective_expiry_ms: i64,
}

/// Bounded wire reasons carry no token, arbitrary caller string or crypto error text.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum DpopRefusal {
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
    #[error("host crypto unavailable")]
    CryptoUnavailable,
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
    #[error("request timestamp outside window or below watermark")]
    RequestTime,
    #[error("request ID must be UUIDv7 and equal proof jti")]
    RequestId,
    #[error("access token hash does not match")]
    TokenHash,
    #[error("invalid operation body")]
    OperationBody,
    #[error("signed intent does not match received operation")]
    Intent,
    #[error("invalid trusted policy or clock")]
    Policy,
}
