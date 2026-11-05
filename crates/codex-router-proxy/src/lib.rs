//! Loopback proxy boundary for codex-router.

pub mod account_selection;
pub(crate) mod claude_edge;
mod claude_route_diagnostics;
mod credential_runtime;
pub mod db_write_actor;
pub mod headers;
pub mod http_sse;
pub mod local_auth;
pub mod maintenance_actor;
pub mod provider_error;
pub mod routes;
mod secret_store_factory;
pub mod server;
pub mod session_account_affinity_cache;
pub mod telemetry;
#[cfg(test)]
mod test_log_capture;
pub mod upstream;
pub mod websocket;

/// Returns this crate's package name.
#[must_use]
pub const fn package_name() -> &'static str {
    "codex-router-proxy"
}

#[cfg(test)]
#[path = "proxy_tests.rs"]
mod tests;
