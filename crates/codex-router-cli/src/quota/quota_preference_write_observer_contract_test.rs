//! Actual shared-store outcomes and final values independently check the observer's facts.
use super::*;
use crate::quota::quota_group_preferences::SqliteQuotaPreferences;
use agent_router_quota_tui::{QuotaGroupPreferences, QuotaUiPreferenceStore};
use std::time::Duration;

#[tokio::test]
async fn preference_write_observer_consumes_actual_shared_store_success_and_failure_after_return() {
    for available in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let path = if available {
            root.path().to_path_buf()
        } else {
            root.path().join("missing-parent")
        };
        let observer = PreferenceWriteObserver::default();
        let store = observer.scope(SqliteQuotaPreferences::open(&path)).await;
        let future = store.save_group_facet(QuotaGroupFacet::Provider);
        assert!(
            observer.facts().unwrap().is_empty(),
            "no actual store return has occurred"
        );
        let result = tokio::time::timeout(Duration::from_secs(3), future)
            .await
            .unwrap();
        let facts = observer.validated_facts().unwrap();
        assert_eq!(
            facts,
            [PreferenceWriteFact {
                ordinal: 1,
                facet: QuotaGroupFacet::Provider,
                result: if result.is_ok() {
                    PreferenceWriteResult::Saved
                } else {
                    PreferenceWriteResult::Failed
                }
            }]
        );
        assert_eq!(result.is_ok(), available);
        assert_eq!(
            store.load_group_facet().await,
            if available {
                QuotaGroupFacet::Provider
            } else {
                QuotaGroupFacet::None
            }
        );
        store.close().await;
        let outside = SqliteQuotaPreferences::open(&path).await;
        let _ = outside.save_group_facet(QuotaGroupFacet::Status).await;
        outside.close().await;
        assert_eq!(
            observer.facts().unwrap(),
            facts,
            "adapter construction outside the task scope is unobserved"
        );
    }
}

#[tokio::test]
async fn terminal_preference_writer_saves_final_native_choice_once_and_ignores_post_quit_choice() {
    let root = tempfile::tempdir().unwrap();
    let observer = PreferenceWriteObserver::default();
    let store = observer
        .scope(SqliteQuotaPreferences::open(root.path()))
        .await;
    let (preferences, writer) = QuotaGroupPreferences::load(Arc::new(store.clone())).await;
    let retained_snapshot_handle = preferences.clone();
    preferences.remember(QuotaGroupFacet::Provider);
    preferences.finish();
    drop(preferences);
    retained_snapshot_handle.remember(QuotaGroupFacet::Status);
    retained_snapshot_handle.finish();
    tokio::time::timeout(Duration::from_secs(3), writer.run())
        .await
        .expect("owner-signaled final write drains while a snapshot handle remains")
        .unwrap();
    assert_eq!(
        observer.validated_facts().unwrap(),
        [PreferenceWriteFact {
            ordinal: 1,
            facet: QuotaGroupFacet::Provider,
            result: PreferenceWriteResult::Saved
        }]
    );
    assert_eq!(
        store.load_group_facet().await,
        QuotaGroupFacet::Provider,
        "the post-Quit choice never replaces the final accepted value"
    );
    assert_eq!(
        retained_snapshot_handle.current_facet(),
        QuotaGroupFacet::Provider
    );
    store.close().await;
}

#[tokio::test]
async fn preference_write_receipt_failure_cannot_be_accepted_as_complete_evidence() {
    let root = tempfile::tempdir().unwrap();
    let observer = PreferenceWriteObserver::with_sink(Arc::new(|_| {
        Err("private write receipt refused".to_owned())
    }));
    let store = observer
        .scope(SqliteQuotaPreferences::open(root.path()))
        .await;
    store
        .save_group_facet(QuotaGroupFacet::Provider)
        .await
        .unwrap();
    assert_eq!(store.load_group_facet().await, QuotaGroupFacet::Provider);
    assert!(
        observer
            .validated_facts()
            .unwrap_err()
            .contains("private write receipt refused")
    );
    store.close().await;
}
