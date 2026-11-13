//! The provider proxy store path under a Router root.
use std::path::{Path, PathBuf};

pub const PROVIDER_PROXY_DATABASE_FILE_NAME: &str = "provider-proxy.sqlite";

/// Returns the provider proxy database under its current name.
#[must_use]
pub fn provider_proxy_database_path(router_root: &Path) -> PathBuf {
    router_root.join(PROVIDER_PROXY_DATABASE_FILE_NAME)
}

#[cfg(test)]
#[path = "provider_proxy_store_test.rs"]
mod tests;
