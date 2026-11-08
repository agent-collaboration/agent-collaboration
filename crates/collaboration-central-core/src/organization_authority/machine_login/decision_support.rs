//! What every machine-login decision shares: its clock reading, its injected
//! UUIDv7 source and the change records it writes.
use super::enrollment_port::MachineEnrollmentFailure;
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

    pub fn record(
        &self,
        request: &str,
        kind: OrganizationChangeKind,
        origin: &OrganizationChangeOrigin,
        target: String,
        time: DecisionTime,
    ) -> Result<OrganizationChangeRecord, MachineEnrollmentFailure> {
        Ok(OrganizationChangeRecord {
            id: self.next()?,
            request_id: ChangeRequestId::try_from(request.to_owned())
                .map_err(|_| MachineEnrollmentFailure::UnrecordableRequest)?,
            kind,
            origin: origin.clone(),
            target_id: target,
            created_at_millis: time.now_ms,
        })
    }
}
