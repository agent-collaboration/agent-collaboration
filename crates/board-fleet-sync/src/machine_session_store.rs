//! The library's resume state is retained by the Router's existing FleetCache.
use fleet_sync_session::{ClientSessionState, SessionClientStore, StoreError};
use message_board_storage::{FleetCache, RouterLocalStorageError};
use std::sync::Arc;

pub struct MachineSessionStore {
    cache: Arc<FleetCache>,
}

impl MachineSessionStore {
    /// A view over the Router's one Fleet database; the Host opens it, imports any
    /// older runtime files into it once, and closes it after every user has stopped.
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
