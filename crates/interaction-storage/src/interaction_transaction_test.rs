//! The interaction store's one transaction function signals only after a commit that
//! changed rows, on the Turso writer.
use super::TransactionStart;
use crate::InteractionStore;
use sqlite_storage_maintenance::CommitPoint;
use std::sync::Arc;
use tokio::sync::watch;

const THREAD_ID: &str = "019a5f2e-7c00-7000-8000-000000000001";

async fn open_store() -> (tempfile::TempDir, InteractionStore) {
    let directory = tempfile::tempdir().expect("interaction fixture directory");
    let store = InteractionStore::open(directory.path().join("interaction.sqlite"))
        .await
        .expect("interaction store opens");
    (directory, store)
}

async fn insert_route(connection: &mut sqlx_turso::TursoConnection) {
    sqlx::query::<sqlx_turso::Turso>(
        "INSERT INTO approval_routes(thread_id,route_json,last_used_at_ms) VALUES (?,'{}',1)",
    )
    .bind(THREAD_ID)
    .execute(connection)
    .await
    .expect("row is written inside the transaction");
}

async fn stored_routes(store: &InteractionStore) -> i64 {
    sqlx::query_scalar::<sqlx_turso::Turso, i64>("SELECT COUNT(*) FROM approval_routes")
        .fetch_one(&mut *store.writer.lock().await)
        .await
        .expect("routes are countable")
}

fn bumps(commits: &mut watch::Receiver<u64>) -> u64 {
    if commits.has_changed().unwrap_or(false) {
        *commits.borrow_and_update()
    } else {
        0
    }
}

#[tokio::test]
async fn a_committed_write_bumps_once_after_its_rows_are_visible() {
    let (_directory, store) = open_store().await;
    let mut commits = store.subscribe_commits();

    {
        let mut connection = store.writer.lock().await;
        let mut transaction = store
            .transaction(&mut connection, TransactionStart::Immediate)
            .await
            .unwrap();
        insert_route(&mut transaction).await;
        transaction.commit().await.unwrap();
    }

    assert_eq!(bumps(&mut commits), 1);
    assert_eq!(stored_routes(&store).await, 1);
}

#[tokio::test]
async fn a_known_rollback_after_a_real_write_does_not_bump() {
    let (_directory, store) = open_store().await;
    let mut commits = store.subscribe_commits();

    {
        let mut connection = store.writer.lock().await;
        let mut transaction = store
            .transaction(&mut connection, TransactionStart::Immediate)
            .await
            .unwrap();
        insert_route(&mut transaction).await;
        transaction.rollback().await.unwrap();
    }

    assert_eq!(bumps(&mut commits), 0);
    assert_eq!(stored_routes(&store).await, 0);
}

#[tokio::test]
async fn reads_and_writes_that_change_nothing_do_not_bump() {
    let (_directory, store) = open_store().await;
    let mut commits = store.subscribe_commits();

    {
        let mut connection = store.writer.lock().await;
        let mut read = store
            .transaction(&mut connection, TransactionStart::Deferred)
            .await
            .unwrap();
        let _: i64 =
            sqlx::query_scalar::<sqlx_turso::Turso, i64>("SELECT COUNT(*) FROM approval_routes")
                .fetch_one(&mut *read)
                .await
                .unwrap();
        read.commit().await.unwrap();
        let mut unchanged = store
            .transaction(&mut connection, TransactionStart::Immediate)
            .await
            .unwrap();
        sqlx::query::<sqlx_turso::Turso>("DELETE FROM approval_routes WHERE thread_id=?")
            .bind(THREAD_ID)
            .execute(&mut *unchanged)
            .await
            .unwrap();
        unchanged.commit().await.unwrap();
    }

    assert_eq!(bumps(&mut commits), 0);
}

#[tokio::test]
async fn a_one_statement_write_signals_like_any_other_write() {
    let (_directory, store) = open_store().await;
    let mut commits = store.subscribe_commits();

    store
        .execute_as_transaction(
            sqlx::query::<sqlx_turso::Turso>(
                "INSERT INTO approval_routes(thread_id,route_json,last_used_at_ms) VALUES (?,'{}',1)",
            )
            .bind(THREAD_ID),
        )
        .await
        .unwrap();
    let inserted = bumps(&mut commits);
    store
        .execute_as_transaction(sqlx::query::<sqlx_turso::Turso>(
            "DELETE FROM approval_routes WHERE thread_id='none'",
        ))
        .await
        .unwrap();

    assert_eq!(inserted, 1);
    assert_eq!(bumps(&mut commits), 0);
}

async fn cancel_a_write_held_at(point: CommitPoint) -> (i64, u64) {
    let (directory, store) = open_store().await;
    let store = Arc::new(store);
    let mut commits = store.subscribe_commits();
    let mut pause = store.commit_signal().pause_next_commit_at(point);
    let writer = {
        let store = Arc::clone(&store);
        tokio::spawn(async move {
            let mut connection = store.writer.lock().await;
            let mut transaction = store
                .transaction(&mut connection, TransactionStart::Immediate)
                .await
                .unwrap();
            insert_route(&mut transaction).await;
            transaction.commit().await
        })
    };

    pause.reached().await;
    writer.abort();
    assert!(writer.await.unwrap_err().is_cancelled());

    let routes = stored_routes(&store).await;
    let bumped = bumps(&mut commits);
    drop(directory);
    (routes, bumped)
}

/// Cancelled after the guard armed and before the backend COMMIT was invoked: the
/// dropped transaction rolls back on the writer's next use, and the store bumps once.
#[tokio::test]
async fn a_write_cancelled_before_commit_is_invoked_rolls_back_and_bumps_once() {
    let (routes, bumped) = cancel_a_write_held_at(CommitPoint::BeforeCommitInvoked).await;

    assert_eq!(routes, 0);
    assert_eq!(bumped, 1);
}

/// Cancelled after the backend COMMIT returned and before the signal settled: the row is
/// durable and the dropped guard still wakes subscribers.
#[tokio::test]
async fn a_write_cancelled_after_commit_returned_is_durable_and_bumps_once() {
    let (routes, bumped) = cancel_a_write_held_at(CommitPoint::AfterCommitReturned).await;

    assert_eq!(routes, 1);
    assert_eq!(bumped, 1);
}
