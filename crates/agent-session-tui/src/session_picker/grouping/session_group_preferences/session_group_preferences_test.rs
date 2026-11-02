use crate::session_picker::grouping::session_group_preferences::*;
use std::sync::Mutex;
use tokio::sync::Notify;
use tokio::sync::oneshot;

#[tokio::test]
async fn group_preferences_unknown_text_becomes_none_and_next_change_replaces_it() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join(SESSION_UI_PREFERENCES_FILENAME);
    let raw = ui_preference_store::UiPreferenceStore::open_and_migrate(&database).await;
    raw.save(SCREEN, GROUP_BY, "removed-facet").await.unwrap();
    raw.close().await;
    let store = Arc::new(SqliteSessionUiPreferences::open_and_migrate(directory.path()).await);
    let (mut preference, writer) = SessionGroupPreferences::load(store.clone()).await;
    assert_eq!(preference.facet(), SessionGroupFacet::None);
    preference.choose(SessionGroupFacet::Day);
    assert_eq!(preference.facet(), SessionGroupFacet::Day);
    drop(preference);
    writer.run().await;
    assert_eq!(store.load_group_facet().await, SessionGroupFacet::Day);
}

#[tokio::test]
async fn group_preferences_corrupt_file_and_failed_save_keep_the_runs_choice() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join(SESSION_UI_PREFERENCES_FILENAME);
    std::fs::write(&database, b"corrupt preferences").unwrap();
    let store = Arc::new(SqliteSessionUiPreferences::open_and_migrate(directory.path()).await);
    let (mut preference, writer) = SessionGroupPreferences::load(store).await;
    assert_eq!(preference.facet(), SessionGroupFacet::None);
    preference.choose(SessionGroupFacet::Machine);
    assert_eq!(preference.facet(), SessionGroupFacet::Machine);
    drop(preference);
    writer.run().await;
    assert_eq!(std::fs::read(&database).unwrap(), b"corrupt preferences");
}

struct HeldSaveStore {
    started: Mutex<Option<oneshot::Sender<()>>>,
    release: Notify,
    saved: Mutex<Vec<SessionGroupFacet>>,
}
impl UiPreferenceStore for HeldSaveStore {
    fn load_group_facet(&self) -> BoxFuture<'_, SessionGroupFacet> {
        Box::pin(async { SessionGroupFacet::Day })
    }
    fn save_group_facet(
        &self,
        facet: SessionGroupFacet,
    ) -> BoxFuture<'_, Result<(), UiPreferenceSaveError>> {
        Box::pin(async move {
            self.saved.lock().unwrap().push(facet);
            let first = self.started.lock().unwrap().take();
            if let Some(started) = first {
                started.send(()).unwrap();
                self.release.notified().await;
            }
            Err(UiPreferenceSaveError)
        })
    }
}

#[tokio::test]
async fn group_preferences_input_does_not_wait_for_save_and_writer_drains_latest_change() {
    let (started, saving) = oneshot::channel();
    let store = Arc::new(HeldSaveStore {
        started: Mutex::new(Some(started)),
        release: Notify::new(),
        saved: Mutex::new(Vec::new()),
    });
    let (mut preference, writer) = SessionGroupPreferences::load(store.clone()).await;
    assert_eq!(preference.facet(), SessionGroupFacet::Day);
    let task = tokio::spawn(writer.run());
    preference.choose(SessionGroupFacet::Provider);
    saving.await.unwrap();
    preference.choose(SessionGroupFacet::Machine);
    assert_eq!(preference.facet(), SessionGroupFacet::Machine);
    store.release.notify_one();
    drop(preference);
    task.await.unwrap();
    assert_eq!(
        *store.saved.lock().unwrap(),
        [SessionGroupFacet::Provider, SessionGroupFacet::Machine]
    );
}

#[tokio::test]
async fn group_preferences_use_only_the_supplied_service_directory() {
    let root = tempfile::tempdir().unwrap();
    let production = root.path().join(".codex-router/agent-communication");
    let debug = root.path().join(".codex-router-debug/agent-communication");
    std::fs::create_dir_all(&production).unwrap();
    std::fs::create_dir_all(&debug).unwrap();
    let production_store = SqliteSessionUiPreferences::open_and_migrate(&production).await;
    let debug_store = SqliteSessionUiPreferences::open_and_migrate(&debug).await;
    production_store
        .save_group_facet(SessionGroupFacet::Folder)
        .await
        .unwrap();
    debug_store
        .save_group_facet(SessionGroupFacet::Provider)
        .await
        .unwrap();
    assert_eq!(
        production_store.load_group_facet().await,
        SessionGroupFacet::Folder
    );
    assert_eq!(
        debug_store.load_group_facet().await,
        SessionGroupFacet::Provider
    );
    assert!(!root.path().join("agent-proxy-ui.sqlite").exists());
}
