//! The registrar asks its machine's connection; transport and authority live outside it.
use std::{future::Future, pin::Pin};

use fleet_identity_protocol::{
    CheckSessionStart, EndFleetSession, RegisterFleetSession, RegisterFleetSessionResult,
    SessionStartCheck,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FleetSessionConnectionFailure {
    #[error("machine connection unavailable")]
    Unavailable,
    #[error("machine connection returned a malformed session answer")]
    MalformedReply,
}

pub type FleetSessionRequest<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, FleetSessionConnectionFailure>> + Send + 'a>>;

pub trait FleetSessionConnection: Send + Sync {
    /// A failed or malformed check never permits provider creation.
    fn check_start<'a>(
        &'a self,
        request: &'a CheckSessionStart,
    ) -> FleetSessionRequest<'a, SessionStartCheck>;

    /// The registrar has durably recorded this exact request before sending it.
    fn register<'a>(
        &'a self,
        request: &'a RegisterFleetSession,
    ) -> FleetSessionRequest<'a, RegisterFleetSessionResult>;

    /// The registrar retires local admission before reporting an archived or deleted session.
    fn end<'a>(&'a self, request: &'a EndFleetSession) -> FleetSessionRequest<'a, ()>;
}
