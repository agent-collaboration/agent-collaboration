//! Test-only ports for exercising FleetRuntime on stock in-memory SQLite.
#![forbid(unsafe_code)]

mod manual_clock;
pub mod signin_fixtures;
mod sqlite_ports;

pub use fleet_runtime::{HeldPause, PausePoint, PausePoints};
pub use manual_clock::ManualClock;
pub use sqlite_ports::{InMemoryCentralStore, InMemoryWorkspaceStores};
