//! M2's named local lease stand-in; M4 replaces it with project_lease_holder.
//!
//! **Superseded (2026-10-16).** Granting leases inside the Router is central behavior written a
//! second time. Under the one-core design, a Router's lease comes only from central's lease
//! registry over its machine session (requirements U69, U20). See `docs/specs/2026-10-16-central-one-core/program-design.md`.
use chrono::{DateTime, Utc};
use message_board::{EpochNumber, ProjectId};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub enum GrantResult {
    Holder {
        epoch: EpochNumber,
        deadline: Instant,
    },
    Expired {
        expired_at: DateTime<Utc>,
    },
    NotHolder {
        current: Option<EpochNumber>,
    },
    Unavailable,
}

#[derive(Default)]
pub struct LocalLeaseAuthority {
    overrides: RwLock<BTreeMap<ProjectId, GrantResult>>,
}
impl LocalLeaseAuthority {
    pub async fn grant(
        &self,
        project: &ProjectId,
    ) -> Result<GrantResult, message_board::NumericValidationError> {
        if let Some(grant) = self.overrides.read().await.get(project) {
            return Ok(grant.clone());
        }
        Ok(GrantResult::Holder {
            epoch: EpochNumber::try_from(1)?,
            deadline: Instant::now() + Duration::from_secs(4 * 60 * 60),
        })
    }

    /// The stand-in's explicit proof control; production composition never calls it.
    pub async fn set_grant(&self, project: ProjectId, grant: GrantResult) {
        self.overrides.write().await.insert(project, grant);
    }
}
