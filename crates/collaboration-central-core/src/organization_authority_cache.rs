//! An org runtime's in-memory view of its own stamp, shared by all its calls.
//!
//! The stored statement is checked against the sign-in keys the first time the
//! object needs it after a process start, and its expiry is rechecked on the
//! injected clock on every use. A stamp that fails the check refuses every
//! command except a provisioning call that can replace it.
use crate::organization_authority::{
    FleetOrganizationStatementRefusal, SignInVerification, StoredStampRead,
    VerifiedFleetOrganizationStatement,
};
use durable_steps::BoxFuture;
use fleet_identity_protocol::UnixSeconds;
use runtime_primitives::SyncTime;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StampHealth {
    Unchecked,
    Unprovisioned,
    Verified {
        issued_at: UnixSeconds,
        expires_at: UnixSeconds,
    },
    Refused,
}

/// What a guarded call may rely on right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StampStanding {
    Unprovisioned,
    Valid,
    Refused,
}

pub(crate) struct OrganizationAuthority {
    check: RwLock<Arc<dyn SignInVerification>>,
    health: Mutex<StampHealth>,
}

impl Default for OrganizationAuthority {
    fn default() -> Self {
        Self {
            check: RwLock::new(Arc::new(UnconfiguredStatementCheck)),
            health: Mutex::new(StampHealth::Unchecked),
        }
    }
}

impl OrganizationAuthority {
    pub(crate) fn install_check(&self, check: Arc<dyn SignInVerification>) {
        if let Ok(mut current) = self.check.write() {
            *current = check;
        }
        self.forget();
    }

    pub(crate) fn check(&self) -> Option<Arc<dyn SignInVerification>> {
        self.check.read().ok().map(|check| check.clone())
    }

    /// Unchecked means this process has not yet looked at the stored stamp.
    pub(crate) fn needs_check(&self) -> bool {
        self.health
            .lock()
            .map(|health| *health == StampHealth::Unchecked)
            .unwrap_or(true)
    }

    /// Record the outcome of checking what the store holds.
    pub(crate) fn record(
        &self,
        stored: &StoredStampRead,
        verified: Option<
            Result<&VerifiedFleetOrganizationStatement, FleetOrganizationStatementRefusal>,
        >,
    ) {
        let next = match (stored, verified) {
            (StoredStampRead::Unprovisioned, _) => StampHealth::Unprovisioned,
            (StoredStampRead::Stamped(_), Some(Ok(statement))) => StampHealth::Verified {
                issued_at: statement.issued_at(),
                expires_at: statement.expires_at(),
            },
            // An unavailable key source leaves the next call to check again,
            // instead of refusing the organization until a restart.
            (
                StoredStampRead::Stamped(_),
                Some(Err(FleetOrganizationStatementRefusal::VerificationUnavailable(_))),
            ) => StampHealth::Unchecked,
            (StoredStampRead::Stamped(_), _) => StampHealth::Refused,
        };
        if let Ok(mut health) = self.health.lock() {
            *health = next;
        }
    }

    pub(crate) fn forget(&self) {
        if let Ok(mut health) = self.health.lock() {
            *health = StampHealth::Unchecked;
        }
    }

    /// The stamp's standing at the caller's sampled time.
    pub(crate) fn standing(&self, now: SyncTime) -> StampStanding {
        let Ok(health) = self.health.lock() else {
            return StampStanding::Refused;
        };
        match *health {
            StampHealth::Unprovisioned => StampStanding::Unprovisioned,
            StampHealth::Verified {
                issued_at,
                expires_at,
            } => {
                let seconds = now.unix_millis().div_euclid(1000);
                if seconds < issued_at.0 || seconds >= expires_at.0 {
                    StampStanding::Refused
                } else {
                    StampStanding::Valid
                }
            }
            StampHealth::Unchecked | StampHealth::Refused => StampStanding::Refused,
        }
    }
}

/// A host that configured no sign-in trust can never accept a stamp.
struct UnconfiguredStatementCheck;
impl SignInVerification for UnconfiguredStatementCheck {
    fn verify_stored_statement_facts<'a>(
        &'a self,
        _statement: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedFleetOrganizationStatement, FleetOrganizationStatementRefusal>>
    {
        Box::pin(std::future::ready(Err(
            FleetOrganizationStatementRefusal::InvalidConfiguration,
        )))
    }

    fn verify_statement<'a>(
        &'a self,
        _statement: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedFleetOrganizationStatement, FleetOrganizationStatementRefusal>>
    {
        Box::pin(std::future::ready(Err(
            FleetOrganizationStatementRefusal::InvalidConfiguration,
        )))
    }
    fn verify_person_request<'a>(
        &'a self,
        _input: &'a crate::person_request::PersonRequestInput,
    ) -> BoxFuture<
        'a,
        Result<
            crate::person_request::VerifiedPersonRequest,
            crate::person_request::PersonRequestRefusal,
        >,
    > {
        Box::pin(std::future::ready(Err(
            crate::person_request::PersonRequestRefusal::Policy,
        )))
    }
    fn sign_in_service(&self) -> Option<message_board::ServiceId> {
        None
    }
}
