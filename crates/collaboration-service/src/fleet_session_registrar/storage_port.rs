//! Atomic registrar facts stay at the private Router state owner.
use std::{future::Future, pin::Pin};

use codex_router_fleet_state::fleet_session_registration::{
    FleetSessionRegistrationRecord, SessionBindingAddress,
};
use collaboration_protocol::OperationId;
use message_board::SessionRef;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FleetSessionStorageFailure {
    #[error("session registration storage unavailable")]
    Unavailable,
    #[error("stored session registration is invalid")]
    InvalidRecord,
    #[error("session registration conflicts with an existing record")]
    Conflict,
    #[error("session registration publication is unconfirmed")]
    PublicationUnknown,
}
pub type FleetSessionStorageRequest<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, FleetSessionStorageFailure>> + Send + 'a>>;

pub enum RegistrationInsert {
    Inserted,
    Existing(Box<FleetSessionRegistrationRecord>),
}

pub trait FleetSessionRegistrationRepository: Send + Sync {
    fn operation<'a>(
        &'a self,
        operation: &'a OperationId,
    ) -> FleetSessionStorageRequest<'a, Option<FleetSessionRegistrationRecord>>;
    fn conversation<'a>(
        &'a self,
        session: &'a SessionRef,
    ) -> FleetSessionStorageRequest<'a, Option<FleetSessionRegistrationRecord>>;
    fn binding<'a>(
        &'a self,
        address: &'a SessionBindingAddress,
    ) -> FleetSessionStorageRequest<'a, Option<FleetSessionRegistrationRecord>>;
    fn insert<'a>(
        &'a self,
        record: &'a FleetSessionRegistrationRecord,
    ) -> FleetSessionStorageRequest<'a, RegistrationInsert>;
    /// Applies only while the full expected immutable input and lifecycle still match.
    fn replace<'a>(
        &'a self,
        expected: &'a FleetSessionRegistrationRecord,
        next: &'a FleetSessionRegistrationRecord,
    ) -> FleetSessionStorageRequest<'a, bool>;
    /// Startup may resend recorded registration bodies; it never recreates conversations.
    fn reconcile_candidates<'a>(
        &'a self,
    ) -> FleetSessionStorageRequest<'a, Vec<FleetSessionRegistrationRecord>>;
}
