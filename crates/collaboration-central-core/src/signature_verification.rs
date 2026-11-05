//! Signature verification needs no object runtime, storage or socket handles.
use crate::{SignatureValidity, VerifyFailure};
use std::future::Future;

pub trait Ed25519Verification: Clone + Send + Sync + 'static {
    fn verify_ed25519(
        &self,
        public_key: &[u8],
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send;
}
