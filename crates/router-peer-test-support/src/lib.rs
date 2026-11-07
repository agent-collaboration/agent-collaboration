//! Scripted Router and peer fixtures shared by product tests.

pub mod global_claude_fixture;
pub mod native_app_server;
pub mod owned_peer_fixture;
pub mod router_health;
pub mod signal_recording;

/// Shared error boundary for Router and peer test fixtures.
pub type FixtureResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;
