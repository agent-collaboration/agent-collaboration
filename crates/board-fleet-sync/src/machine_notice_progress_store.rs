//! Consumer progress remains independent of the library's transport acknowledgement.
use crate::machine_session_store::cache_error;
use fleet_sync_protocol::NoticePosition;
use fleet_sync_session::StoreError;
use message_board_storage::FleetCache;
use std::path::Path;

pub struct MachineNoticeProgressStore {
    cache: FleetCache,
}

impl MachineNoticeProgressStore {
    pub async fn open(router_directory: &Path) -> Result<Self, StoreError> {
        tokio::fs::create_dir_all(router_directory)
            .await
            .map_err(|error| StoreError::Unavailable {
                message: error.to_string(),
            })?;
        let cache = FleetCache::open(&router_directory.join("fleet-cache.sqlite"))
            .await
            .map_err(cache_error)?;
        cache
            .import_legacy_runtime(router_directory)
            .await
            .map_err(cache_error)?;
        Ok(Self { cache })
    }
    pub async fn load(&self) -> Result<NoticePosition, StoreError> {
        self.cache
            .machine_notice_progress()
            .await
            .map_err(cache_error)
    }
    pub async fn save(&self, handled_through: NoticePosition) -> Result<(), StoreError> {
        self.cache
            .save_machine_notice_progress(handled_through)
            .await
            .map_err(cache_error)
    }
}
