//! Native signature mechanics for the isolated person-DPoP prototype.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    SignatureValidity, VerifyFailure,
    person_dpop_prototype::{DpopCrypto, Ed25519Jwk, Es256Jwk},
};
use ring::signature::{ECDSA_P256_SHA256_FIXED, ED25519, UnparsedPublicKey};
use std::future::{Future, ready};

pub struct NativePersonDpopCrypto;

impl DpopCrypto for NativePersonDpopCrypto {
    fn verify_issuer<'a>(
        &'a self,
        key: &'a Ed25519Jwk,
        signed: &'a [u8],
        signature: &'a [u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + 'a {
        ready(decode(&key.x).map(|public_key| {
            validity(UnparsedPublicKey::new(&ED25519, public_key).verify(signed, signature))
        }))
    }

    fn verify_proof<'a>(
        &'a self,
        key: &'a Es256Jwk,
        signed: &'a [u8],
        signature: &'a [u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + 'a {
        ready(verify_es256(key, signed, signature))
    }
}

fn verify_es256(
    key: &Es256Jwk,
    signed: &[u8],
    signature: &[u8],
) -> Result<SignatureValidity, VerifyFailure> {
    let mut public_key = vec![4]; // SEC1 uncompressed point: 04 || X || Y.
    public_key.extend(decode(&key.x)?);
    public_key.extend(decode(&key.y)?);
    Ok(validity(
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, public_key).verify(signed, signature),
    ))
}

fn decode(component: &str) -> Result<Vec<u8>, VerifyFailure> {
    URL_SAFE_NO_PAD
        .decode(component)
        .map_err(|_| VerifyFailure::UnsupportedKey)
}

fn validity(result: Result<(), ring::error::Unspecified>) -> SignatureValidity {
    match result {
        Ok(()) => SignatureValidity::Valid,
        Err(_) => SignatureValidity::Invalid,
    }
}
