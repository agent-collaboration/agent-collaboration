//! Real optional SQLite preferences reopen independently from quota/account state.
use super::*;
use agent_router_quota_tui::QuotaGroupPreferences;
use std::sync::Arc;

#[tokio::test]
async fn quota_grouping_round_trips_through_its_own_shared_store_file() {
    let root = tempfile::tempdir().unwrap();
    for facet in [
        QuotaGroupFacet::Provider,
        QuotaGroupFacet::Status,
        QuotaGroupFacet::None,
    ] {
        let store = SqliteQuotaPreferences::open(root.path()).await;
        let (preferences, writer) = QuotaGroupPreferences::load(Arc::new(store.clone())).await;
        preferences.remember(facet);
        preferences.finish();
        drop(preferences);
        writer.run().await.unwrap();
        store.close().await;
        let reopened = SqliteQuotaPreferences::open(root.path()).await;
        assert_eq!(reopened.load_group_facet().await, facet);
        reopened.close().await;
        let paths = std::fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(paths, [std::ffi::OsString::from("agent-proxy-ui.sqlite")]);
    }
}

#[tokio::test]
async fn unknown_and_unreadable_quota_preferences_fall_back_without_replacing_the_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("agent-proxy-ui.sqlite");
    let raw = ui_preference_store::UiPreferenceStore::open_and_migrate(&path).await;
    raw.save("quota", "group-by", "future-facet").await.unwrap();
    raw.save("sessions", "group-by", "provider").await.unwrap();
    raw.close().await;
    let store = SqliteQuotaPreferences::open(root.path()).await;
    assert_eq!(store.load_group_facet().await, QuotaGroupFacet::None);
    store.close().await;
    let corrupt_root = tempfile::tempdir().unwrap();
    let corrupt_path = corrupt_root.path().join("agent-proxy-ui.sqlite");
    std::fs::write(&corrupt_path, b"retained damaged UI preferences").unwrap();
    let store = SqliteQuotaPreferences::open(corrupt_root.path()).await;
    assert_eq!(store.load_group_facet().await, QuotaGroupFacet::None);
    assert_eq!(
        store.save_group_facet(QuotaGroupFacet::Provider).await,
        Err(QuotaPreferenceSaveError)
    );
    store.close().await;
    assert_eq!(
        std::fs::read(corrupt_path).unwrap(),
        b"retained damaged UI preferences"
    );
}

#[tokio::test]
async fn unavailable_quota_store_does_not_discard_the_run_choice() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteQuotaPreferences::open(&root.path().join("missing-parent")).await;
    let (preferences, writer) = QuotaGroupPreferences::load(Arc::new(store.clone())).await;
    assert_eq!(preferences.current_facet(), QuotaGroupFacet::None);
    preferences.remember(QuotaGroupFacet::Status);
    assert_eq!(preferences.current_facet(), QuotaGroupFacet::Status);
    preferences.finish();
    drop(preferences);
    writer.run().await.unwrap();
    store.close().await;
    assert!(!root.path().join("missing-parent").exists());
}
