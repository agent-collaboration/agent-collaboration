//! Shared result type for central conformance cases.
#![forbid(unsafe_code)]

/// The error-boxing result every shared fixture and host scenario returns.
pub type ConformanceResult<TValue> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;
