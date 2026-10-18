//! Publication order is independent of source-clock freshness and lease authority.
use crate::{CentralError, RegistryTime};
use collaboration_replication_protocol::{
    DirectoryLease, Epoch, PieceDigest, ProjectId, RequestId,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::num::NonZeroU64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RegistryRunId(RequestId);
impl RegistryRunId {
    pub fn new(
        value: impl Into<String>,
    ) -> Result<Self, collaboration_replication_protocol::IdentityError> {
        RequestId::new(value).map(Self)
    }
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseVersion {
    epoch: Epoch,
    incarnation: RegistryIncarnation,
    change: NonZeroU64,
}
impl LeaseVersion {
    pub fn new(
        epoch: Epoch,
        incarnation: RegistryIncarnation,
        change: u64,
    ) -> Result<Self, LeaseVersionError> {
        Ok(Self {
            epoch,
            incarnation,
            change: NonZeroU64::new(change).ok_or(LeaseVersionError { field: "change" })?,
        })
    }
    #[must_use]
    pub const fn epoch(self) -> Epoch {
        self.epoch
    }
    #[must_use]
    pub const fn incarnation(self) -> RegistryIncarnation {
        self.incarnation
    }
    #[must_use]
    pub const fn change(self) -> u64 {
        self.change.get()
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
    pub fn new(
        value: impl Into<String>,
    ) -> Result<Self, collaboration_replication_protocol::DigestError> {
        PieceDigest::try_from(value.into()).map(Self)
    }
    pub fn of(
        project: &ProjectId,
        lease: &DirectoryLease,
        as_of: RegistryTime,
    ) -> Result<Self, CentralError> {
        let bytes = serde_json::to_vec(&(project, lease, as_of)).map_err(|error| {
            CentralError::Encoding {
                message: error.to_string(),
            }
        })?;
        Self::new(format!("{:x}", Sha256::digest(bytes))).map_err(|error| CentralError::Encoding {
            message: error.to_string(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum LeaseUpdateResult {
    Applied,
    Duplicate,
    Older { stored: LeaseVersion },
    Fenced { fence: RegistryIncarnation },
    Conflict { stored: LeaseVersion },
}
