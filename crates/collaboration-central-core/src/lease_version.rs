//! Order and digest values for the committed library lease projection.
use fleet_sync_protocol::PieceDigest;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RegistryIncarnation(NonZeroU64);
impl RegistryIncarnation {
    pub fn new(value: u64) -> Result<Self, LeaseVersionError> {
        NonZeroU64::new(value).map(Self).ok_or(LeaseVersionError {
            field: "incarnation",
        })
    }
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("lease version {field} must be positive")]
pub struct LeaseVersionError {
    pub field: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LeaseContentDigest(PieceDigest);
impl LeaseContentDigest {
    pub fn new(value: impl Into<String>) -> Result<Self, fleet_sync_protocol::DigestError> {
        PieceDigest::try_from(value.into()).map(Self)
    }
}
