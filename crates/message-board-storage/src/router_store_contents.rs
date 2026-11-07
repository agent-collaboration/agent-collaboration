//! Test support: what a Router's own stores may hold. A Router keeps a cache of
//! central's records, never central's authority itself.
use crate::RouterLocalStorageError;
use sqlx::ConnectOptions;
use std::{collections::BTreeSet, path::Path};

/// Every table the Router's Fleet database may hold, its migration history included.
pub const ROUTER_FLEET_TABLES: &[&str] = &[
    "_sqlx_migrations",
    "cached_fleet_records",
    "fleet_cache_checkpoint",
    "fleet_cache_rebuild_records",
    "update_steps",
    "fleet_outbox_requests",
    "fleet_project_creation_inputs",
    "fleet_cursor_signing_key",
];

/// Tables of central's own authority (its feed, directory and decisions), which a
/// Router root must never hold in any store.
pub const CENTRAL_AUTHORITY_TABLES: &[&str] = &[
    "fleet_feed",
    "fleet_feed_state",
    "fleet_workspaces",
    "fleet_boards",
    "fleet_project_directory",
    "fleet_request_decisions",
    "machine_enrollments",
];

/// The tables in one store, read without the store's lock and without creating it.
pub async fn store_table_names(path: &Path) -> Result<BTreeSet<String>, RouterLocalStorageError> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await?;
    let names = sqlx::query_scalar::<sqlx_turso::Turso, String>(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(&mut connection)
    .await?;
    sqlx::Connection::close(connection).await?;
    Ok(names.into_iter().collect())
}

/// The first store directly in `directory` that holds a table of central's authority,
/// with that table, or `None` when every store is a Router store.
pub async fn store_holding_central_authority(
    directory: &Path,
) -> Result<Option<(std::path::PathBuf, String)>, RouterLocalStorageError> {
    let entries = std::fs::read_dir(directory).map_err(sqlx::Error::Io)?;
    for entry in entries {
        let path = entry.map_err(sqlx::Error::Io)?.path();
        if path
            .extension()
            .is_none_or(|extension| extension != "sqlite")
        {
            continue;
        }
        for table in store_table_names(&path).await? {
            if CENTRAL_AUTHORITY_TABLES.contains(&table.as_str()) {
                return Ok(Some((path, table)));
            }
        }
    }
    Ok(None)
}
