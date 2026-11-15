//! Caller-sampled decision time, typed policy settings and audit identities.
use crate::identity_contract::TokenLifetimeSeconds;
use crate::organization_change_rows::{
    ChangeRequestId, OrganizationChangeKind, OrganizationChangeOrigin, OrganizationChangeRecord,
};
use central_storage::StoreFailure;
use fleet_identity_protocol::UnixSeconds;
use runtime_primitives::IdSource;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DecisionTime {
    pub now_ms: i64,
}
impl DecisionTime {
    pub(crate) const fn seconds(self) -> UnixSeconds {
        UnixSeconds(self.now_ms.div_euclid(1000))
    }
}

pub(super) struct DecisionIds<'a>(pub &'a dyn IdSource);
impl DecisionIds<'_> {
    pub(super) fn next(&self) -> Result<Uuid, StoreFailure> {
        let id = self.0.new_id();
        if !runtime_primitives::uuid_v7(id) {
            return Err(StoreFailure::Unavailable);
        }
        Uuid::parse_str(&id.to_string()).map_err(|_| StoreFailure::Unavailable)
    }
    pub(super) fn record(
        &self,
        originating: &str,
        kind: OrganizationChangeKind,
        origin: &OrganizationChangeOrigin,
        target: String,
        time: DecisionTime,
    ) -> Result<OrganizationChangeRecord, StoreFailure> {
        let request_id =
            ChangeRequestId::try_from(format!("{originating}:{}:{target}", kind.as_str()))?;
        Ok(OrganizationChangeRecord {
            id: self.next()?,
            request_id,
            kind,
            origin: origin.clone(),
            target_id: target,
            created_at_millis: time.now_ms,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachineLoginSettings {
    pub grant_lifetime: TokenLifetimeSeconds,
}
impl MachineLoginSettings {
    pub(crate) const IDLE_EXPIRY_SECONDS: i64 = 30 * 24 * 60 * 60;
    pub const fn new(grant_lifetime: TokenLifetimeSeconds) -> Self {
        Self { grant_lifetime }
    }
    /// Compose central's fixed 600-second machine grant policy.
    pub const fn production() -> Self {
        Self::new(TokenLifetimeSeconds::MACHINE_LOGIN_FIXED)
    }
}

#[cfg(test)]
#[path = "decision_support_test.rs"]
mod tests;
