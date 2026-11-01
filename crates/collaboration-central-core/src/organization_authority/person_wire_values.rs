//! Person identity timestamps follow their producer's safe JSON integer bounds.

use fleet_identity_protocol::UnixSeconds;
use serde::Deserialize;

const MAX_SAFE_JSON_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Deserialize)]
#[serde(try_from = "i64")]
pub(super) struct PersonWireSeconds(UnixSeconds);

impl TryFrom<i64> for PersonWireSeconds {
    type Error = InvalidPersonWireTime;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        validate_epoch(value)?;
        Ok(Self(UnixSeconds(value)))
    }
}

impl PersonWireSeconds {
    pub(super) const fn into_unix_seconds(self) -> UnixSeconds {
        self.0
    }
}

/// Validate checkedAt without retaining a timestamp a guard might compare.
#[derive(Deserialize)]
#[serde(try_from = "i64")]
pub(super) struct ProviderCheckTime;

impl TryFrom<i64> for ProviderCheckTime {
    type Error = InvalidPersonWireTime;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        validate_epoch(value)?;
        Ok(Self)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("person proof time must be a nonnegative safe JSON integer")]
pub(super) struct InvalidPersonWireTime;

fn validate_epoch(value: i64) -> Result<(), InvalidPersonWireTime> {
    if !(0..=MAX_SAFE_JSON_INTEGER).contains(&value) {
        return Err(InvalidPersonWireTime);
    }
    Ok(())
}
