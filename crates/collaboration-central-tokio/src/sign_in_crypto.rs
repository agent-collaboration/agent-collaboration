//! Native signature mechanics; core owns the sign-in key and token profiles.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    Ed25519Verification, SignatureValidity, VerifyFailure,
    person_request::{Es256Verification, ProofJwk},
};
use ring::signature::{ECDSA_P256_SHA256_FIXED, ED25519, UnparsedPublicKey};

#[derive(Clone)]
pub(crate) struct NativeEd25519;

impl Ed25519Verification for NativeEd25519 {
    async fn verify_ed25519(
        &self,
        public_key: &[u8],
        signed: &[u8],
        signature: &[u8],
    ) -> Result<SignatureValidity, VerifyFailure> {
        Ok(
            if UnparsedPublicKey::new(&ED25519, public_key)
                .verify(signed, signature)
                .is_ok()
            {
                SignatureValidity::Valid
            } else {
                SignatureValidity::Invalid
            },
        )
    }
}

#[derive(Clone)]
pub(crate) struct NativeEs256;

impl Es256Verification for NativeEs256 {
    async fn verify_es256(
        &self,
        key: &ProofJwk,
        signed: &[u8],
        signature: &[u8],
    ) -> Result<SignatureValidity, VerifyFailure> {
        let decode = |component: &str| {
            URL_SAFE_NO_PAD
                .decode(component)
                .map_err(|_| VerifyFailure::UnsupportedKey)
        };
        // SEC1 uncompressed point: 04 || X || Y.
        let mut public_key = vec![4];
        public_key.extend(decode(&key.x)?);
        public_key.extend(decode(&key.y)?);
        Ok(
            if UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, public_key)
                .verify(signed, signature)
                .is_ok()
            {
                SignatureValidity::Valid
            } else {
                SignatureValidity::Invalid
            },
        )
    }
}

#[cfg(test)]
#[path = "sign_in_crypto_test.rs"]
mod tests;
