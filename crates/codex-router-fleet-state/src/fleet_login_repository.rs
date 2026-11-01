//! Workspace-internal persistence contract; callers never select a database engine.
use crate::fleet_login::{FleetLoginCandidate, SavedFleetLogin};
use fleet_identity_protocol::{EnrollmentId, OrganizationId};
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FleetLoginStorageFailure {
    #[error("fleet store unavailable")]
    Unavailable,
    #[error("stored fleet login is invalid: {field}")]
    InvalidRecord { field: &'static str },
    #[error("fleet login belongs to another organization")]
    OrganizationMismatch { organization_id: OrganizationId },
    #[error("fleet login publication is unconfirmed")]
    PublicationUnknown,
}

pub type FleetLoginStorageRequest<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, FleetLoginStorageFailure>> + Send + 'a>>;

pub trait FleetLoginRepository: Send + Sync {
    fn load_fleet_login(&self) -> FleetLoginStorageRequest<'_, Option<SavedFleetLogin>>;

    /// Reads and replaces one row atomically. The first omitted environment is
    /// physical; a later omission keeps the current row's value. Organization
    /// mismatches leave the row unchanged. Errors do not imply no write occurred.
    fn replace_fleet_login<'a>(
        &'a self,
        candidate: &'a FleetLoginCandidate,
    ) -> FleetLoginStorageRequest<'a, SavedFleetLogin>;

    /// A stale enrollment must never delete a newer login.
    fn delete_fleet_login<'a>(
        &'a self,
        expected_enrollment: &'a EnrollmentId,
    ) -> FleetLoginStorageRequest<'a, bool>;
}
