//! Signature verification needs no object runtime, storage or socket handles.
use crate::SignatureValidity;
use std::future::Future;

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
