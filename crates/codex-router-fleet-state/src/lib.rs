//! This Router's fleet records in `fleet.sqlite`, under the Router root.
//!
//! Owns `fleet_login` and `fleet_session_registrations` with its own migrations.
//! This database stores no proxy account state and no token bytes.

pub mod fleet_login;
pub mod fleet_login_repository;
mod fleet_migrations;
mod sqlite;
mod sqlite_login;
pub use fleet_migrations::FleetStateOpenError;
pub use sqlite::FleetStateStore;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

/// The fleet database owned by this crate, separate from proxy account storage.
pub const FLEET_DATABASE_FILENAME: &str = "fleet.sqlite";
