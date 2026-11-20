//! Signature verification needs no object runtime, storage or socket handles.
use std::future::Future;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureValidity {
    Valid,
    Invalid,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifyFailure {
    UnsupportedKey,
    Crypto,
}

pub trait Ed25519Verification: Clone + Send + Sync + 'static {
    fn verify_ed25519(
        &self,
        public_key: &[u8],
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send;
}

/// ES256 proof signatures; the host supplies the mechanics.
pub trait Es256Verification: Clone + Send + Sync + 'static {
    fn verify_es256(
        &self,
        key: &ProofJwk,
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send;
}

/// Public P-256 proof key only; a private `d` or any other member is refused.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProofJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
}
