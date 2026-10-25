//! Native RSA verification after the core has selected an allowed key.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{AccessJwk, SignatureValidity, VerifyFailure};
use ring::{rsa::PublicKeyComponents, signature::RSA_PKCS1_2048_8192_SHA256};

pub(crate) fn verify_rs256_signature(
    key: &AccessJwk,
    signed: &[u8],
    signature: &[u8],
) -> Result<SignatureValidity, VerifyFailure> {
    let modulus = URL_SAFE_NO_PAD
        .decode(&key.n)
        .map_err(|_| VerifyFailure::UnsupportedKey)?;
    let exponent = URL_SAFE_NO_PAD
        .decode(&key.e)
        .map_err(|_| VerifyFailure::UnsupportedKey)?;
    let key = PublicKeyComponents {
        n: unsigned_component(&modulus)?,
        e: unsigned_component(&exponent)?,
    };
    Ok(
        if key
            .verify(&RSA_PKCS1_2048_8192_SHA256, signed, signature)
            .is_ok()
        {
            SignatureValidity::Valid
        } else {
            SignatureValidity::Invalid
        },
    )
}

fn unsigned_component(bytes: &[u8]) -> Result<&[u8], VerifyFailure> {
    let start = bytes
        .iter()
        .position(|byte| *byte != 0)
        .ok_or(VerifyFailure::UnsupportedKey)?;
    bytes.get(start..).ok_or(VerifyFailure::UnsupportedKey)
}
