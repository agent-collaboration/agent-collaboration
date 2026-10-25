//! The same RSA key profile applies before either runtime verifies a signature.

use crate::{AccessJwk, AccessRefusal};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

pub(crate) fn validate_access_key_profile(key: &AccessJwk) -> Result<(), AccessRefusal> {
    let modulus = URL_SAFE_NO_PAD
        .decode(&key.n)
        .map_err(|_| AccessRefusal::InvalidKey)?;
    let exponent = URL_SAFE_NO_PAD
        .decode(&key.e)
        .map_err(|_| AccessRefusal::InvalidKey)?;
    let modulus = unsigned_component(&modulus)?;
    let exponent = unsigned_component(&exponent)?;
    let first = modulus.first().ok_or(AccessRefusal::InvalidKey)?;
    let bits = modulus.len() * 8 - first.leading_zeros() as usize;
    if !(2048..=4096).contains(&bits) || modulus.last().is_none_or(|byte| byte % 2 == 0) {
        return Err(AccessRefusal::InvalidKey);
    }
    let exponent = exponent
        .iter()
        .try_fold(0_u64, |value, byte| {
            value.checked_mul(256)?.checked_add(u64::from(*byte))
        })
        .ok_or(AccessRefusal::InvalidKey)?;
    if !(3..=(1_u64 << 33) - 1).contains(&exponent) || exponent % 2 == 0 {
        return Err(AccessRefusal::InvalidKey);
    }
    Ok(())
}

fn unsigned_component(bytes: &[u8]) -> Result<&[u8], AccessRefusal> {
    let start = bytes
        .iter()
        .position(|byte| *byte != 0)
        .ok_or(AccessRefusal::InvalidKey)?;
    bytes.get(start..).ok_or(AccessRefusal::InvalidKey)
}
