//! Directory lease observations have their own ordered version and source clock.
use crate::EpochNumber;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct RegistryIncarnation(NonZeroU64);
impl RegistryIncarnation {
    pub fn new(value: u64) -> Result<Self, LeaseVersionError> {
        NonZeroU64::new(value).map(Self).ok_or(LeaseVersionError {
            field: "incarnation",
        })
    }
    pub const fn value(self) -> u64 {
        self.0.get()
    }
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseVersion {
    epoch: EpochNumber,
    incarnation: RegistryIncarnation,
    change: NonZeroU64,
}
impl LeaseVersion {
    pub fn new(
        epoch: EpochNumber,
        incarnation: RegistryIncarnation,
        change: u64,
    ) -> Result<Self, LeaseVersionError> {
        Ok(Self {
            epoch,
            incarnation,
            change: NonZeroU64::new(change).ok_or(LeaseVersionError { field: "change" })?,
        })
    }
    pub const fn epoch(self) -> EpochNumber {
        self.epoch
    }
    pub const fn incarnation(self) -> RegistryIncarnation {
        self.incarnation
    }
    pub const fn change(self) -> u64 {
        self.change.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("lease version {field} must be positive")]
pub struct LeaseVersionError {
    pub field: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriterObservation {
    pub lease_version: LeaseVersion,
    pub as_of: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lease_version_orders_epoch_before_incarnation_and_change() {
        let earlier = LeaseVersion::new(
            2.try_into().unwrap(),
            RegistryIncarnation::new(99).unwrap(),
            99,
        )
        .unwrap();
        let later = LeaseVersion::new(
            3.try_into().unwrap(),
            RegistryIncarnation::new(1).unwrap(),
            1,
        )
        .unwrap();
        assert!(later > earlier);
        assert!(
            LeaseVersion::new(
                3.try_into().unwrap(),
                RegistryIncarnation::new(2).unwrap(),
                1
            )
            .unwrap()
                > later
        );
        assert!(
            LeaseVersion::new(
                3.try_into().unwrap(),
                RegistryIncarnation::new(1).unwrap(),
                2
            )
            .unwrap()
                > later
        );
        assert!(
            serde_json::from_str::<LeaseVersion>(r#"{"epoch":1,"incarnation":0,"change":1}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<LeaseVersion>(r#"{"epoch":1,"incarnation":1,"change":0}"#)
                .is_err()
        );
    }
}
