//! The library's resume state is retained by the Router's existing FleetCache.
use fleet_sync_session::{ClientSessionState, SessionClientStore, StoreError};
use message_board_storage::{FleetCache, RouterLocalStorageError};
use std::{path::Path, sync::Arc};

pub struct MachineSessionStore {
    cache: Arc<FleetCache>,
}

impl MachineSessionStore {
    pub async fn open(router_directory: &Path) -> Result<Self, StoreError> {
        tokio::fs::create_dir_all(router_directory)
            .await
            .map_err(|error| StoreError::Unavailable {
                message: error.to_string(),
            })?;
        let cache = Arc::new(
            FleetCache::open(&router_directory.join("fleet-cache.sqlite"))
                .await
                .map_err(cache_error)?,
        );
        cache
            .import_legacy_runtime(router_directory)
            .await
            .map_err(cache_error)?;
        Ok(Self::from_cache(cache))
    }

    pub fn from_cache(cache: Arc<FleetCache>) -> Self {
        Self { cache }
    }

    pub fn cache(&self) -> Arc<FleetCache> {
        self.cache.clone()
    }
}

impl SessionClientStore for MachineSessionStore {
    async fn load(&self) -> Result<Option<ClientSessionState>, StoreError> {
        self.cache
            .machine_session_state()
            .await
            .map_err(cache_error)
    }
    async fn save(&self, state: &ClientSessionState) -> Result<(), StoreError> {
        self.cache
            .save_machine_session_state(state)
            .await
            .map_err(cache_error)
    }
}

pub(crate) fn cache_error(error: RouterLocalStorageError) -> StoreError {
    match error {
        RouterLocalStorageError::InvalidRecordKey { field } => StoreError::Corrupt {
            field: field.to_owned(),
        },
        other => StoreError::Unavailable {
            message: other.to_string(),
        },
    }
}
