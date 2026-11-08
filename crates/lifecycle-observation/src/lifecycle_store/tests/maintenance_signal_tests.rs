//! Maintenance wakes journal readers only when it removed rows.
use super::{LifecycleStore, ObservationJournal, identity, observation, test_database_path};

const DAY_SECONDS: i64 = 86_400;

fn signalled(commits: &mut tokio::sync::watch::Receiver<u64>) -> bool {
    let changed = commits.has_changed().unwrap_or(false);
    commits.borrow_and_update();
    changed
}

#[tokio::test]
async fn maintenance_signals_only_when_it_removed_history() {
    let path = test_database_path("maintenance-signal");
    let journal = ObservationJournal::open(&path, identity())
        .await
        .unwrap_or_else(|error| panic!("journal should open: {error}"));
    let store = LifecycleStore::new(journal);
    store
        .append(&observation(), 1)
        .await
        .unwrap_or_else(|error| panic!("fixture row should append: {error}"));
    let mut commits = store.commits.subscribe();

    store
        .maintain(2)
        .await
        .unwrap_or_else(|error| panic!("quiet maintenance should succeed: {error}"));
    let quiet_signalled = signalled(&mut commits);
    store
        .maintain(100 * DAY_SECONDS)
        .await
        .unwrap_or_else(|error| panic!("expiring maintenance should succeed: {error}"));
    let expiry_signalled = signalled(&mut commits);
    let bounds = store
        .bounds()
        .await
        .unwrap_or_else(|error| panic!("bounds should read: {error}"));

    assert!(!quiet_signalled, "nothing was due, so readers stay asleep");
    assert!(expiry_signalled, "removed history must wake readers");
    assert!(
        bounds.earliest_sequence > bounds.last_sequence,
        "the old record was removed: {bounds:?}"
    );
    store.close().await;
    let _ = std::fs::remove_file(path);
}
