//! One Router owner admits starts and registers the provider's actual conversation.
mod binding_admission;
mod connection_port;
mod registration_lifecycle;
mod start_admission;
mod storage_port;

pub use binding_admission::BoundFleetSession;
pub use connection_port::{
    FleetSessionConnection, FleetSessionConnectionFailure, FleetSessionRequest,
};
pub use registration_lifecycle::RegistrationCompletion;
pub use start_admission::{
    FleetSessionRegistrar, FleetSessionRegistrarProps, PrepareSessionStart, RegistrarFailure,
    SessionBindingEntropy, SessionBindingEntropyUnavailable, SessionBindingListener,
    SessionRegistrationMode, SessionRegistrationModeSource,
};
pub use storage_port::{
    FleetSessionRegistrationRepository, FleetSessionStorageFailure, FleetSessionStorageRequest,
    RegistrationInsert,
};

#[cfg(test)]
#[path = "fleet_session_registrar/policy_tests.rs"]
mod policy_tests;
