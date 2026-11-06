//! Owned child capture and scenario finalization for caller-supplied test resources.

pub mod owned_cli_process;
pub mod scenario_finalization;

/// Error boundary shared by owned child and finalization scenarios.
pub type FixtureResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;
