use crate::RegistryTime;
use collaboration_replication_protocol::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedCopy {
    pub digest: ImportDigest,
    pub import_epoch: Epoch,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnheldProject {
    pub project: ProjectId,
    pub import_epoch: Epoch,
    pub imported_head: ReplicationPosition,
    pub schema_version: u64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportSeedResult {
    pub answer: ProjectImportAnswer,
    pub unheld: Option<UnheldProject>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum DirectoryUpdate {
    ClaimGranted {
        project: ProjectId,
        attempt: RequestId,
        lease: DirectoryLease,
        lease_version: crate::LeaseVersion,
        digest: crate::LeaseContentDigest,
        as_of: RegistryTime,
    },
    Summary {
        project: ProjectId,
        summary: ProjectSummary,
        position: ReplicationPosition,
        acked_at: RegistryTime,
    },
    Lease {
        project: ProjectId,
        lease: DirectoryLease,
        lease_version: crate::LeaseVersion,
        digest: crate::LeaseContentDigest,
        as_of: RegistryTime,
    },
}
impl DirectoryUpdate {
    pub fn lease(
        project: ProjectId,
        lease: DirectoryLease,
        lease_version: crate::LeaseVersion,
        as_of: RegistryTime,
    ) -> Result<Self, crate::CentralError> {
        let digest = crate::LeaseContentDigest::of(&project, &lease, as_of)?;
        Ok(Self::Lease {
            project,
            lease,
            lease_version,
            digest,
            as_of,
        })
    }
    pub fn claim_granted(
        project: ProjectId,
        attempt: RequestId,
        lease: DirectoryLease,
        lease_version: crate::LeaseVersion,
        as_of: RegistryTime,
    ) -> Result<Self, crate::CentralError> {
        let digest = crate::LeaseContentDigest::of(&project, &lease, as_of)?;
        Ok(Self::ClaimGranted {
            project,
            attempt,
            lease,
            lease_version,
            digest,
            as_of,
        })
    }
    pub(crate) fn key(&self) -> String {
        match self {
            Self::ClaimGranted {
                project, attempt, ..
            } => format!("claim:{}:{}", project.as_str(), attempt.as_str()),
            Self::Summary { project, .. } => format!("summary:{}", project.as_str()),
            Self::Lease { project, .. } => format!("lease:{}", project.as_str()),
        }
    }
}
