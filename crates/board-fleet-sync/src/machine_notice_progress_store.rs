//! Consumer progress remains independent of the library's transport acknowledgement.
use crate::machine_session_store::cache_error;
use fleet_sync_protocol::NoticePosition;
use fleet_sync_session::StoreError;
use message_board_storage::FleetCache;

pub struct MachineNoticeProgressStore {
    cache: FleetCache,
}

impl MachineNoticeProgressStore {
    /// A view over the Router's one Fleet database, which the Host owns and closes.
    #[must_use]
    pub const fn from_cache(cache: FleetCache) -> Self {
        Self { cache }
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
