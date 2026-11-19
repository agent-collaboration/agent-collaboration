//! Shared sign-in fixtures for both central hosts' test suites.
#![forbid(unsafe_code)]
#[cfg(all(feature = "signin-test-support", not(target_arch = "wasm32")))]
pub mod signin_fixtures;

/// The error-boxing result every shared fixture and host scenario returns.
pub type ConformanceResult<TValue> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;
