//! Read the JS byte count without worker0.8.7's wasm32 usize narrowing.
use worker::{Error, Result, Storage};

pub(crate) fn storage_bytes(storage: &Storage) -> Result<u64> {
    checked_storage_bytes(storage.as_raw().sql().database_size())
}

fn checked_storage_bytes(value: f64) -> Result<u64> {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > MAX_SAFE_INTEGER {
        return Err(Error::RustError(
            "invalid SQL databaseSize measurement".into(),
        ));
    }
    // Validated non-negative safe integers are represented exactly in u64.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "The finite integral measurement is checked within 0..=2^53-1 above."
    )]
    let byte_count = value as u64;
    Ok(byte_count)
}

#[cfg(test)]
mod tests {
    use super::checked_storage_bytes;

    #[test]
    fn measurements_above_wasm32_usize_remain_exact() {
        for (number, expected) in [
            (0.0, 0),
            (4_294_967_295.0, 4_294_967_295),
            (4_294_967_296.0, 4_294_967_296),
            (5_368_709_120.0, 5_368_709_120),
            (9_007_199_254_740_991.0, 9_007_199_254_740_991),
        ] {
            assert_eq!(checked_storage_bytes(number).ok(), Some(expected));
        }
    }

    #[test]
    fn malformed_or_inexact_measurements_are_refused() {
        for number in [
            -1.0,
            0.5,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            9_007_199_254_740_992.0,
        ] {
            assert!(checked_storage_bytes(number).is_err());
        }
    }
}
