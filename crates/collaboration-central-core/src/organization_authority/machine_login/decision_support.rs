//! What every machine-login decision shares: its clock reading, its injected
//! UUIDv7 source and the change records it writes.
use super::enrollment_port::MachineEnrollmentFailure;
use crate::identity_contract::TokenLifetimeSeconds;
use crate::organization_authority::{
    ChangeRequestId, OrganizationChangeKind, OrganizationChangeOrigin, OrganizationChangeRecord,
};
use durable_steps::Uuid;
use fleet_identity_protocol::UnixSeconds;
use runtime_primitives::{Clock, IdSource};

/// The applying transaction's clock, read once inside the transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecisionTime {
    pub now_ms: i64,
}

impl DecisionTime {
    #[must_use]
    pub fn sample(clock: &dyn Clock) -> Self {
        Self {
            now_ms: clock.now().unix_millis(),
        }
    }

    #[must_use]
    pub const fn seconds(self) -> UnixSeconds {
        UnixSeconds(self.now_ms.div_euclid(1000))
    }
}

/// Mints row and record IDs from the host's ID source, refusing anything but UUIDv7.
pub(super) struct DecisionIds<'a>(pub &'a dyn IdSource);

impl DecisionIds<'_> {
    pub fn next(&self) -> Result<Uuid, MachineEnrollmentFailure> {
        let id = self.0.new_id();
        if runtime_primitives::uuid_v7(id) {
            Ok(id)
        } else {
            Err(MachineEnrollmentFailure::InvalidIdSource)
        }
    }

    /// One decision writes several records, and the store keeps one record per
    /// request key. So each record's key is `<originating id>:<kind>:<target>`:
    /// a retried decision writes the same keys, and no two records of one
    /// decision share one. Every originating id and target here is a UUID, so a
    /// key is at most 90 characters.
    pub fn record(
        &self,
        originating: &str,
        kind: OrganizationChangeKind,
        origin: &OrganizationChangeOrigin,
        target: String,
        time: DecisionTime,
    ) -> Result<OrganizationChangeRecord, MachineEnrollmentFailure> {
        let request_key = format!("{originating}:{}:{target}", kind.as_str());
        Ok(OrganizationChangeRecord {
            id: self.next()?,
            request_id: ChangeRequestId::try_from(request_key)
                .map_err(|_| MachineEnrollmentFailure::UnrecordableRequest)?,
            kind,
            origin: origin.clone(),
            target_id: target,
            created_at_millis: time.now_ms,
        })
    }
}

/// Central's machine-login settings. `grant_lifetime` stands in for the
/// organization's `SetTokenLifetime` value until that command has a home.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachineLoginSettings {
    pub grant_lifetime: TokenLifetimeSeconds,
    /// An enrollment expires this long after its last Grant (R40).
    pub idle_expiry_seconds: i64,
}

impl MachineLoginSettings {
    pub const IDLE_EXPIRY_SECONDS: i64 = 30 * 24 * 60 * 60;

    #[must_use]
    pub const fn new(grant_lifetime: TokenLifetimeSeconds) -> Self {
        Self {
            grant_lifetime,
            idle_expiry_seconds: Self::IDLE_EXPIRY_SECONDS,
        }
    }
}
