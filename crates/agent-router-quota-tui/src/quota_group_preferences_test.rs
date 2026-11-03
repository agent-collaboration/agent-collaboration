//! Choices remain synchronous while the retained writer awaits a real store completion.
use super::*;
use std::{sync::Mutex, time::Duration};
use tokio::sync::oneshot;

struct HeldPreferenceStore {
    entered: Mutex<Option<oneshot::Sender<QuotaGroupFacet>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
    saved: Mutex<Vec<QuotaGroupFacet>>,
    fail: bool,
}
impl QuotaUiPreferenceStore for HeldPreferenceStore {
    fn load_group_facet(&self) -> QuotaPreferenceFuture<'_, QuotaGroupFacet> {
        Box::pin(async { QuotaGroupFacet::None })
    }
    fn save_group_facet(
        &self,
        facet: QuotaGroupFacet,
    ) -> QuotaPreferenceFuture<'_, Result<(), QuotaPreferenceSaveError>> {
        Box::pin(async move {
            let entered = self.entered.lock().unwrap().take();
            let release = self.release.lock().unwrap().take();
            if let Some(entered) = entered {
                entered.send(facet).unwrap();
            }
            if let Some(release) = release {
                release.await.unwrap();
            }
            self.saved.lock().unwrap().push(facet);
            if self.fail {
                Err(QuotaPreferenceSaveError)
            } else {
                Ok(())
            }
        })
    }
}

#[tokio::test]
async fn held_preference_save_keeps_latest_run_choice_and_drains_after_producers_close() {
    let (entered, entry) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let store = Arc::new(HeldPreferenceStore {
        entered: Mutex::new(Some(entered)),
        release: Mutex::new(Some(released)),
        saved: Mutex::new(Vec::new()),
        fail: false,
    });
    let (preferences, writer) = QuotaGroupPreferences::load(store.clone()).await;
    assert_eq!(preferences.current_facet(), QuotaGroupFacet::None);
    preferences.remember(QuotaGroupFacet::Provider);
    let writer = tokio::spawn(writer.run());
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), entry)
            .await
            .unwrap()
            .unwrap(),
        QuotaGroupFacet::Provider
    );
    preferences.remember(QuotaGroupFacet::Status);
    let remounted = preferences.clone();
    assert_eq!(remounted.current_facet(), QuotaGroupFacet::Status);
    assert!(
        store.saved.lock().unwrap().is_empty(),
        "the first save is still held"
    );
    preferences.finish();
    drop(preferences);
    drop(remounted);
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), writer)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        *store.saved.lock().unwrap(),
        [QuotaGroupFacet::Provider, QuotaGroupFacet::Status]
    );
}

#[tokio::test]
async fn preference_save_failure_keeps_the_choice_without_retry() {
    let store = Arc::new(HeldPreferenceStore {
        entered: Mutex::new(None),
        release: Mutex::new(None),
        saved: Mutex::new(Vec::new()),
        fail: true,
    });
    let (preferences, writer) = QuotaGroupPreferences::load(store.clone()).await;
    preferences.remember(QuotaGroupFacet::Status);
    preferences.remember(QuotaGroupFacet::Status);
    assert_eq!(preferences.current_facet(), QuotaGroupFacet::Status);
    preferences.finish();
    drop(preferences);
    tokio::time::timeout(Duration::from_secs(3), writer.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*store.saved.lock().unwrap(), [QuotaGroupFacet::Status]);
}

#[tokio::test]
async fn terminal_signal_drains_latest_once_with_retained_handles_and_ignores_late_choices() {
    let (entered, entry) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let store = Arc::new(HeldPreferenceStore {
        entered: Mutex::new(Some(entered)),
        release: Mutex::new(Some(released)),
        saved: Mutex::new(Vec::new()),
        fail: false,
    });
    let (preferences, writer) = QuotaGroupPreferences::load(store.clone()).await;
    let retained = preferences.clone();
    preferences.remember(QuotaGroupFacet::Provider);
    let writer = tokio::spawn(writer.run());
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), entry)
            .await
            .unwrap()
            .unwrap(),
        QuotaGroupFacet::Provider
    );
    preferences.remember(QuotaGroupFacet::Status);
    preferences.finish();
    drop(preferences);
    retained.remember(QuotaGroupFacet::None);
    assert_eq!(
        retained.current_facet(),
        QuotaGroupFacet::Status,
        "late choices cannot replace the final accepted facet"
    );
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), writer)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        *store.saved.lock().unwrap(),
        [QuotaGroupFacet::Provider, QuotaGroupFacet::Status]
    );
    retained.remember(QuotaGroupFacet::Provider);
    assert_eq!(retained.current_facet(), QuotaGroupFacet::Status);
}

#[tokio::test]
async fn dropped_producers_without_terminal_signal_are_not_a_successful_drain() {
    let store = Arc::new(HeldPreferenceStore {
        entered: Mutex::new(None),
        release: Mutex::new(None),
        saved: Mutex::new(Vec::new()),
        fail: false,
    });
    let (preferences, writer) = QuotaGroupPreferences::load(store).await;
    drop(preferences);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), writer.run())
            .await
            .unwrap(),
        Err(QuotaPreferenceWriterStopped)
    );
}
