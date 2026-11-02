//! Quota's typed preference port uses only its own optional UI database.
use agent_router_quota_tui::{
    QuotaGroupFacet, QuotaPreferenceFuture, QuotaPreferenceSaveError, QuotaUiPreferenceStore,
};
use std::path::Path;

#[derive(Clone)]
pub(super) struct SqliteQuotaPreferences {
    store: ui_preference_store::UiPreferenceStore,
    #[cfg(all(test, debug_assertions, feature = "quota-reset-test-harness"))]
    observer: Option<observation::PreferenceWriteObserver>,
}

impl SqliteQuotaPreferences {
    pub(super) async fn open(router_root: &Path) -> Self {
        Self {
            store: ui_preference_store::UiPreferenceStore::open_and_migrate(
                &router_root.join("agent-proxy-ui.sqlite"),
            )
            .await,
            #[cfg(all(test, debug_assertions, feature = "quota-reset-test-harness"))]
            observer: observation::current(),
        }
    }
    pub(super) async fn close(self) {
        self.store.close().await;
    }
}

impl QuotaUiPreferenceStore for SqliteQuotaPreferences {
    fn load_group_facet(&self) -> QuotaPreferenceFuture<'_, QuotaGroupFacet> {
        Box::pin(async {
            QuotaGroupFacet::from_stored_value(
                self.store.load("quota", "group-by").await.as_deref(),
            )
        })
    }
    fn save_group_facet(
        &self,
        facet: QuotaGroupFacet,
    ) -> QuotaPreferenceFuture<'_, Result<(), QuotaPreferenceSaveError>> {
        Box::pin(async move {
            let result = self
                .store
                .save("quota", "group-by", facet.stored_value())
                .await;
            #[cfg(all(test, debug_assertions, feature = "quota-reset-test-harness"))]
            if let Some(observer) = &self.observer {
                observer.save_returned(facet, result);
            }
            result.map_err(|_| QuotaPreferenceSaveError)
        })
    }
}

#[cfg(test)]
#[path = "quota_group_preferences_test.rs"]
mod tests;

#[cfg(all(test, debug_assertions, feature = "quota-reset-test-harness"))]
#[path = "quota_preference_write_observation_test_support.rs"]
pub(crate) mod observation;
