//! This Router's fleet records in `fleet.sqlite`, under the Router root.
//!
//! Owns `fleet_login` and `fleet_session_registrations` with its own migrations.
//! This database stores no proxy account state and no token bytes.

pub mod fleet_login;

/// The fleet database owned by this crate, separate from proxy account storage.
pub const FLEET_DATABASE_FILENAME: &str = "fleet.sqlite";

pub mod fleet_session_registration;

pub mod session_registration_repository;
