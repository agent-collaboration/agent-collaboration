//! Where the provider proxy's store lives under a Router root, and the checkpoint
//! taken before the store is renamed from its former name.
//!
//! Every program that opens the store by Router root goes through
//! [`provider_proxy_database_path`]; an explicit `--state-db` path still wins where a
//! command accepts one.

use sqlx::{ConnectOptions as _, Connection as _, sqlite::SqliteConnectOptions};
use std::path::{Path, PathBuf};

/// The store's file name under the Router root.
pub const PROVIDER_PROXY_DATABASE_FILE_NAME: &str = "provider-proxy.sqlite";
/// The name step 0008 renames the store from.
pub const FORMER_PROVIDER_PROXY_DATABASE_FILE_NAME: &str = "state.sqlite";

/// `<router root>/provider-proxy.sqlite`.
#[must_use]
pub fn provider_proxy_database_path(router_root: &Path) -> PathBuf {
    router_root.join(PROVIDER_PROXY_DATABASE_FILE_NAME)
}

/// `<router root>/state.sqlite`, the store's name before step 0008.
#[must_use]
pub fn former_provider_proxy_database_path(router_root: &Path) -> PathBuf {
    router_root.join(FORMER_PROVIDER_PROXY_DATABASE_FILE_NAME)
}

/// The checkpoint before step 0008's rename could not open, checkpoint or close the store.
/// It belongs to the checkpoint alone, so this module does not depend on the store's own
/// error type.
#[derive(Debug, thiserror::Error)]
#[error("provider proxy store checkpoint failed: {0}")]
pub struct ProviderProxyCheckpointError(#[from] sqlx::Error);

/// Moves every committed page from the store's write-ahead log into its database file,
/// then closes. Opens the existing file only: it never creates the store or runs its
/// migrations.
pub async fn checkpoint_provider_proxy_database(
    path: &Path,
) -> Result<(), ProviderProxyCheckpointError> {
    let mut connection = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .connect()
        .await?;
    let checkpointed = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&mut connection)
        .await;
    let closed = connection.close().await;
    checkpointed?;
    Ok(closed?)
}

#[cfg(test)]
#[path = "provider_proxy_store_test.rs"]
mod tests;
