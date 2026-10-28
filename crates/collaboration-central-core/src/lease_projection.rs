//! Committed registry facts keep the enrolled Router identity distinct from its peer.
use crate::{LeaseContentDigest, RegistryIncarnation};
use fleet_sync_lease::{DatabaseLifecycle, RegistrySnapshot};
use fleet_sync_protocol::{DatabaseId, Epoch, TransferId};
use message_board::ServiceId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LeaseProjectionVersion {
    epoch: Epoch,
    incarnation: RegistryIncarnation,
    change: NonZeroU64,
}
impl LeaseProjectionVersion {
    pub(crate) fn incarnation(&self) -> RegistryIncarnation {
        self.incarnation
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "LeaseProjectionFields")]
pub(crate) struct LeaseProjection {
    snapshot: RegistrySnapshot,
    incarnation: RegistryIncarnation,
    holder: Option<ServiceId>,
    digest: LeaseContentDigest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LeaseProjectionFields {
    snapshot: RegistrySnapshot,
    incarnation: RegistryIncarnation,
    holder: Option<ServiceId>,
    digest: LeaseContentDigest,
}

#[derive(Debug, thiserror::Error)]
#[error("invalid committed lease projection: {field}")]
pub(crate) struct InvalidLeaseProjection {
    field: &'static str,
}

impl LeaseProjection {
    pub(crate) fn from_snapshot(
        snapshot: RegistrySnapshot,
        holder: Option<ServiceId>,
        incarnation: RegistryIncarnation,
    ) -> Result<Self, InvalidLeaseProjection> {
        let grant_expected = matches!(snapshot.lifecycle, DatabaseLifecycle::Registered { .. });
        if snapshot.grant.is_some() != grant_expected
            || snapshot.grant.is_some() != holder.is_some()
            || snapshot
                .grant
                .as_ref()
                .is_some_and(|grant| grant.epoch != snapshot.revision.epoch)
        {
            return Err(InvalidLeaseProjection { field: "grant" });
        }
        // Digest only the committed library snapshot and its verified holder mapping.
        let bytes = serde_json::to_vec(&(&snapshot, &holder))
            .map_err(|_| InvalidLeaseProjection { field: "encoding" })?;
        let digest = LeaseContentDigest::new(format!("{:x}", Sha256::digest(bytes)))
            .map_err(|_| InvalidLeaseProjection { field: "digest" })?;
        Ok(Self {
            snapshot,
            incarnation,
            holder,
            digest,
        })
    }

    pub(crate) fn database(&self) -> DatabaseId {
        self.snapshot.database
    }
    pub(crate) fn incarnation(&self) -> RegistryIncarnation {
        self.incarnation
    }
    pub(crate) fn version(&self) -> LeaseProjectionVersion {
        LeaseProjectionVersion {
            epoch: self.snapshot.revision.epoch,
            incarnation: self.incarnation,
            change: self.snapshot.revision.change,
        }
    }
    pub(crate) fn digest(&self) -> &LeaseContentDigest {
        &self.digest
    }
    pub(crate) fn is_unheld(&self) -> bool {
        matches!(self.snapshot.lifecycle, DatabaseLifecycle::Unheld { .. })
    }

    pub(crate) fn directory_metadata(
        &self,
        now: fleet_sync_protocol::SyncTime,
    ) -> Result<fleet_admission::LeaseMetadata, durable_steps::StoreError> {
        use fleet_admission::{DirectoryLease, DirectoryLeaseState, LeaseMetadata};
        let version = self.version();
        let display_time = |time: fleet_sync_protocol::SyncTime| {
            chrono::DateTime::from_timestamp_millis(time.unix_millis()).ok_or_else(|| {
                crate::fleet_record_codec::corrupt(
                    "directory.lease_time",
                    "stored lease time is outside the display range",
                )
            })
        };
        let lease = match &self.snapshot.lifecycle {
            DatabaseLifecycle::Retired { .. } => DirectoryLease::Retired {
                retired_at: display_time(self.snapshot.changed_at)?,
            },
            DatabaseLifecycle::Unheld { .. } | DatabaseLifecycle::Retiring { .. } => {
                DirectoryLease::Unheld
            }
            DatabaseLifecycle::Registered { .. } => {
                let grant = self.snapshot.grant.as_ref().ok_or_else(|| {
                    crate::fleet_record_codec::corrupt(
                        "directory.grant",
                        "registered projection has no grant",
                    )
                })?;
                DirectoryLease::Held {
                    epoch: version.epoch,
                    holder: self.holder.clone().ok_or_else(|| {
                        crate::fleet_record_codec::corrupt(
                            "directory.holder",
                            "grant has no verified service mapping",
                        )
                    })?,
                    state: if grant.expires_at > now {
                        DirectoryLeaseState::Held
                    } else {
                        DirectoryLeaseState::Expired
                    },
                    expires_at: display_time(grant.expires_at)?,
                }
            }
        };
        let invalid_version = || {
            crate::fleet_record_codec::corrupt(
                "directory.lease_version",
                "invalid committed lease version",
            )
        };
        let lease_version = message_board::LeaseVersion::new(
            version
                .epoch
                .value()
                .try_into()
                .map_err(|_| invalid_version())?,
            message_board::RegistryIncarnation::new(version.incarnation.value())
                .map_err(|_| invalid_version())?,
            version.change.get(),
        )
        .map_err(|_| invalid_version())?;
        Ok(LeaseMetadata {
            lease,
            lease_version,
            as_of: display_time(self.snapshot.changed_at)?,
        })
    }
}

impl TryFrom<LeaseProjectionFields> for LeaseProjection {
    type Error = InvalidLeaseProjection;
    fn try_from(fields: LeaseProjectionFields) -> Result<Self, Self::Error> {
        let value = Self::from_snapshot(fields.snapshot, fields.holder, fields.incarnation)?;
        if value.digest != fields.digest {
            return Err(InvalidLeaseProjection { field: "digest" });
        }
        Ok(value)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ClaimGrantedUpdate {
    pub attempt: TransferId,
    pub projection: LeaseProjection,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum LeaseProjectionResult {
    Applied,
    Duplicate,
    Older { stored: LeaseProjectionVersion },
    Fenced { fence: RegistryIncarnation },
    Conflict { stored: LeaseProjectionVersion },
}
