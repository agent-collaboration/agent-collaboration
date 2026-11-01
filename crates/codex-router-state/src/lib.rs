//! Proxy account and routing state, stored in `state.sqlite`.
//!
//! Owns accounts, credential metadata, quota and credit, routing policy,
//! session-account affinity, and client leases. Fleet records belong in
//! `codex-router-fleet-state` and its separate `fleet.sqlite` database.

pub mod account;
mod account_migrations;
pub mod account_routing_policy;
mod account_schema;
pub mod affinity_owner;
pub mod credential_maintenance;
mod credential_maintenance_store;
pub mod credit_store;
pub mod fleet_session_registration;
pub mod quota_snapshot;
pub mod repositories;
pub mod selection_projection;
pub mod session_account_affinity;
pub mod sqlite;
pub mod window_observation;

/// Returns this crate's package name.
#[must_use]
pub const fn package_name() -> &'static str {
    "codex-router-state"
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;

#[cfg(test)]
mod future_send_contract;
