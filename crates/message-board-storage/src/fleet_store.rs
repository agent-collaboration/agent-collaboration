//! The Router's one Fleet database. The Host opens it once, and the Fleet cache, the
//! Fleet outbox and the board cursor key are views over that one writable connection.
use crate::{
    CursorStore, FleetCache, FleetOutbox, RouterLocalStorageError,
    fleet_cache_schema::CACHE_MIGRATOR, router_local_database::RouterLocalDatabase,
};
use std::{path::Path, sync::Arc};

pub struct FleetStore {
    database: Arc<RouterLocalDatabase>,
}

impl FleetStore {
    /// The Fleet database's file in the Router's collaboration directory.
    pub const FILE_NAME: &'static str = "fleet.sqlite";

    /// Opens the Fleet database, creating it when missing. A store this call creates
    /// has nothing old in it, so it records every update step done (see `update_ledger`).
    pub async fn open(path: &Path) -> Result<Self, RouterLocalStorageError> {
        let created = !tokio::fs::try_exists(path).await.map_err(sqlx::Error::Io)?;
        if let Some(directory) = path.parent() {
            tokio::fs::create_dir_all(directory)
                .await
                .map_err(sqlx::Error::Io)?;
        }
        let database = RouterLocalDatabase::open(path, &CACHE_MIGRATOR).await?;
        if created {
            crate::update_ledger::record_catalog_done_in_new_store(&database).await?;
        }
        Ok(Self {
            database: Arc::new(database),
        })
    }

    /// Opens a Fleet database that must already exist, under whatever name it has now;
    /// a missing file is an error, never a new store.
    pub async fn open_existing(path: &Path) -> Result<Self, RouterLocalStorageError> {
        Ok(Self {
            database: Arc::new(RouterLocalDatabase::open_existing(path, &CACHE_MIGRATOR).await?),
        })
    }

    #[must_use]
    pub fn cache(&self) -> FleetCache {
        FleetCache::over(Arc::clone(&self.database))
    }

    #[must_use]
    pub fn outbox(&self) -> FleetOutbox {
        FleetOutbox::over(Arc::clone(&self.database))
    }

    #[must_use]
    pub fn cursors(&self) -> CursorStore {
        CursorStore::over(Arc::clone(&self.database))
    }

    /// A receiver that wakes after each committed transaction that changed Fleet rows.
    #[must_use]
    pub fn subscribe_commits(&self) -> tokio::sync::watch::Receiver<u64> {
        self.database.subscribe_commits()
    }

    /// Closes the one connection; views still held fail their next call.
    pub async fn close(&self) {
        self.database.pool.close().await;
    }
}
