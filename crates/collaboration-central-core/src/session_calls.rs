//! Core-owned read used to assemble Welcome through the caller DAG.
use durable_steps::CallTarget;
use fleet_protocol::{LeaseStatus, PeerId};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct SessionLeaseRegistry;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionLeaseRequest {
    pub session: fleet_session::SessionKey,
    pub request: fleet_protocol::LeaseRequest,
}
impl CallTarget for SessionLeaseRegistry {
    const KIND: &'static str = "central.session-lease";
    type Request = SessionLeaseRequest;
    type Answer = fleet_protocol::LeaseAnswer;
}

#[derive(Serialize, Deserialize)]
pub(crate) struct WelcomeLeases;
impl CallTarget for WelcomeLeases {
    const KIND: &'static str = "central.welcome-leases";
    type Request = PeerId;
    type Answer = Vec<LeaseStatus>;
}
