//! A run owns its facet immediately; its caller separately drives and drains the writer.
use super::session_grouping::SessionGroupFacet;
use futures_util::future::BoxFuture;
use std::{path::Path, sync::Arc};
use tokio::sync::watch;
use ui_preference_store::UiPreferenceSaveError;

pub const SESSION_UI_PREFERENCES_FILENAME: &str = "agent-sessions-ui.sqlite";
const SCREEN: &str = "sessions";
const GROUP_BY: &str = "group-by";

pub trait UiPreferenceStore: Send + Sync {
    fn load_group_facet(&self) -> BoxFuture<'_, SessionGroupFacet>;
    fn save_group_facet(
        &self,
        facet: SessionGroupFacet,
    ) -> BoxFuture<'_, Result<(), UiPreferenceSaveError>>;
}

pub struct SqliteSessionUiPreferences {
    store: ui_preference_store::UiPreferenceStore,
}

impl SqliteSessionUiPreferences {
    /// The caller supplies its existing production or debug service directory.
    pub async fn open_and_migrate(service_directory: &Path) -> Self {
        Self {
            store: ui_preference_store::UiPreferenceStore::open_and_migrate(
                &service_directory.join(SESSION_UI_PREFERENCES_FILENAME),
            )
            .await,
        }
    }
}

impl UiPreferenceStore for SqliteSessionUiPreferences {
    fn load_group_facet(&self) -> BoxFuture<'_, SessionGroupFacet> {
        Box::pin(async {
            self.store
                .load(SCREEN, GROUP_BY)
                .await
                .as_deref()
                .map(SessionGroupFacet::from_stored_value)
                .unwrap_or_default()
        })
    }
    fn save_group_facet(
        &self,
        facet: SessionGroupFacet,
    ) -> BoxFuture<'_, Result<(), UiPreferenceSaveError>> {
        Box::pin(async move {
            self.store
                .save(SCREEN, GROUP_BY, facet.stored_value())
                .await
        })
    }
}

pub struct SessionGroupPreferences {
    facet: SessionGroupFacet,
    changes: watch::Sender<SessionGroupFacet>,
}

pub struct SessionGroupPreferenceWriter {
    store: Arc<dyn UiPreferenceStore>,
    changes: watch::Receiver<SessionGroupFacet>,
}

impl SessionGroupPreferences {
    pub async fn load(store: Arc<dyn UiPreferenceStore>) -> (Self, SessionGroupPreferenceWriter) {
        let facet = store.load_group_facet().await;
        let (sender, receiver) = watch::channel(facet);
        (
            Self {
                facet,
                changes: sender,
            },
            SessionGroupPreferenceWriter {
                store,
                changes: receiver,
            },
        )
    }

    pub const fn facet(&self) -> SessionGroupFacet {
        self.facet
    }

    pub fn choose(&mut self, facet: SessionGroupFacet) {
        if self.facet != facet {
            self.facet = facet;
            self.changes.send_replace(facet);
        }
    }
}

impl SessionGroupPreferenceWriter {
    /// Serial saves preserve the latest choice. Dropping the run drains its final change.
    pub async fn run(mut self) {
        while self.changes.changed().await.is_ok() {
            let facet = *self.changes.borrow_and_update();
            let _ = self.store.save_group_facet(facet).await;
        }
    }
}

#[cfg(test)]
#[path = "session_group_preferences_tests.rs"]
mod tests;
