//! The one transaction function signals only after a commit that changed rows.
use super::{AutomationStore, TransactionStart};
use sqlite_storage_maintenance::CommitPoint;
use std::sync::Arc;
use tokio::sync::Mutex;

const OPERATION_ID: &str = "019a5f2e-7c00-7000-8000-000000000001";

async fn open_store() -> (tempfile::TempDir, AutomationStore) {
    let directory = tempfile::tempdir().expect("automation fixture directory");
    let store = AutomationStore::open(&directory.path().join("automation.sqlite"))
        .await
        .expect("automation store opens");
    (directory, store)
}

async fn insert_remote_wake_request(connection: &mut sqlx::SqliteConnection) {
    sqlx::query("INSERT INTO remote_wake_requests(operation_id,target_json,canonical_request,body_json,valid_until_ms) VALUES(?,'{}',x'00','{}',1)")
        .bind(OPERATION_ID)
        .execute(connection)
        .await
        .expect("row is written inside the transaction");
}

async fn stored_rows(store: &mut AutomationStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM remote_wake_requests")
        .fetch_one(&mut store.connection)
        .await
        .expect("rows are countable")
}

#[tokio::test]
async fn a_committed_write_bumps_once_after_its_rows_are_visible() {
    let (_directory, mut store) = open_store().await;
    let mut commits = store.subscribe_commits();

    let mut transaction = store
        .transaction(TransactionStart::Immediate)
        .await
        .unwrap();
    insert_remote_wake_request(&mut transaction).await;
    transaction.commit().await.unwrap();

    assert!(commits.has_changed().unwrap());
    assert_eq!(*commits.borrow_and_update(), 1);
    assert_eq!(stored_rows(&mut store).await, 1);
}

#[tokio::test]
async fn a_known_rollback_after_a_real_write_does_not_bump() {
    let (_directory, mut store) = open_store().await;
    let commits = store.subscribe_commits();

    let mut transaction = store
        .transaction(TransactionStart::Immediate)
        .await
        .unwrap();
    insert_remote_wake_request(&mut transaction).await;
    transaction.rollback().await.unwrap();

    assert!(!commits.has_changed().unwrap());
    assert_eq!(stored_rows(&mut store).await, 0);
}

#[tokio::test]
async fn a_write_dropped_before_commit_rolls_back_without_bumping() {
    let (_directory, mut store) = open_store().await;
    let commits = store.subscribe_commits();

    let mut transaction = store
        .transaction(TransactionStart::Immediate)
        .await
        .unwrap();
    insert_remote_wake_request(&mut transaction).await;
    drop(transaction);

    assert!(!commits.has_changed().unwrap());
    assert_eq!(stored_rows(&mut store).await, 0);
}

#[tokio::test]
async fn reads_and_writes_that_change_nothing_do_not_bump() {
    let (_directory, mut store) = open_store().await;
    let commits = store.subscribe_commits();

    let mut read = store.transaction(TransactionStart::Deferred).await.unwrap();
    let _: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM remote_wake_requests")
        .fetch_one(&mut *read)
        .await
        .unwrap();
    read.commit().await.unwrap();
    let mut unchanged = store
        .transaction(TransactionStart::Immediate)
        .await
        .unwrap();
    sqlx::query("DELETE FROM remote_wake_requests WHERE operation_id=?")
        .bind(OPERATION_ID)
        .execute(&mut *unchanged)
        .await
        .unwrap();
    unchanged.commit().await.unwrap();

    assert!(!commits.has_changed().unwrap());
}

async fn cancel_a_write_held_at(point: CommitPoint) -> (i64, u64) {
    let (directory, store) = open_store().await;
    let store = Arc::new(Mutex::new(store));
    let mut commits = store.lock().await.subscribe_commits();
    let mut pause = store
        .lock()
        .await
        .commit_signal()
        .pause_next_commit_at(point);
    let writer = {
        let store = Arc::clone(&store);
        tokio::spawn(async move {
            let mut store = store.lock().await;
            let mut transaction = store
                .transaction(TransactionStart::Immediate)
                .await
                .unwrap();
            insert_remote_wake_request(&mut transaction).await;
            transaction.commit().await
        })
    };

    pause.reached().await;
    writer.abort();
    assert!(writer.await.unwrap_err().is_cancelled());

    let rows = stored_rows(&mut *store.lock().await).await;
    let bumps = if commits.has_changed().unwrap() {
        *commits.borrow_and_update()
    } else {
        0
    };
    drop(directory);
    (rows, bumps)
}

/// Cancelled after the guard armed and before the backend COMMIT was invoked: the
/// transaction rolls back, and the store bumps once because it can't know that.
#[tokio::test]
async fn a_write_cancelled_before_commit_is_invoked_rolls_back_and_bumps_once() {
    let (rows, bumps) = cancel_a_write_held_at(CommitPoint::BeforeCommitInvoked).await;

    assert_eq!(rows, 0);
    assert_eq!(bumps, 1);
}

/// Cancelled after the backend COMMIT returned and before the signal settled: the rows
/// are durable, and the dropped guard still wakes subscribers.
#[tokio::test]
async fn a_write_cancelled_after_commit_returned_is_durable_and_bumps_once() {
    let (rows, bumps) = cancel_a_write_held_at(CommitPoint::AfterCommitReturned).await;

    assert_eq!(rows, 1);
    assert_eq!(bumps, 1);
}
