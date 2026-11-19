use agent_automation::{EventId, InstructionText, OperationId};
use runtime_primitives::Clock;
use sqlx::Connection;
use tokio::sync::Mutex;

use super::AutomationRetentionWorker;
use automation_storage::AutomationStore;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

fn manual_clock() -> Arc<router_clock::ManualClock> {
    Arc::new(
        router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
            1792432800000,
        ))
        .expect("fixed retention clock"),
    )
}

#[tokio::test]
async fn minute_worker_prunes_expired_events_without_touching_current_or_instruction_rows() {
    let clock = manual_clock();
    let path = std::env::temp_dir().join(format!(
        "automation-retention-worker-{}.sqlite",
        OperationId::generate().as_str()
    ));
    let mut store = AutomationStore::open(&path)
        .await
        .unwrap_or_else(|error| panic!("automation store should open: {error}"));
    let instruction = store
        .create_instruction(
            &OperationId::generate(),
            &InstructionText::try_from("retained instruction".to_owned())
                .unwrap_or_else(|error| panic!("instruction text should validate: {error}")),
            0,
        )
        .await
        .unwrap_or_else(|error| panic!("instruction should persist: {error}"));
    let now = clock.now().unix_millis();
    let expired = now - 90_i64 * 24 * 60 * 60 * 1000;
    let mut observer = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
    )
    .await
    .unwrap_or_else(|error| panic!("observer should connect: {error}"));
    for timestamp in [expired, now] {
        sqlx::query("INSERT INTO automation_events(event_id,subject_kind,subject_id,event_kind,event_body_json,recorded_at_ms) VALUES (?,'instruction',?,'fixture','{}',?)")
                .bind(EventId::generate().as_str())
                .bind(instruction.instruction_id.as_str())
                .bind(timestamp)
                .execute(&mut observer)
                .await
                .unwrap_or_else(|error| panic!("fixture event should persist: {error}"));
    }

    let shared_store = Arc::new(Mutex::new(store));
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(
        AutomationRetentionWorker::new(Arc::clone(&shared_store), clock.clone())
            .run(shutdown.clone()),
    );
    (async {
        loop {
            let retained: Vec<i64> = sqlx::query_scalar(
                "SELECT recorded_at_ms FROM automation_events ORDER BY recorded_at_ms",
            )
            .fetch_all(&mut observer)
            .await
            .unwrap_or_else(|error| panic!("worker result should query: {error}"));
            if retained == vec![now] {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(
        shared_store
            .lock()
            .await
            .read_instruction(&instruction.instruction_id)
            .await
            .unwrap_or_else(|error| panic!("instruction should remain readable: {error}")),
        instruction
    );

    shutdown.cancel();
    task.await
        .unwrap_or_else(|error| panic!("worker should join: {error}"));
    observer
        .close()
        .await
        .unwrap_or_else(|error| panic!("observer should close: {error}"));
    Arc::try_unwrap(shared_store)
        .unwrap_or_else(|_| panic!("worker should release the store reference"))
        .into_inner()
        .close()
        .await
        .unwrap_or_else(|error| panic!("store should close: {error}"));
    std::fs::remove_file(path)
        .unwrap_or_else(|error| panic!("fixture database should remove: {error}"));
}

#[tokio::test]
async fn automation_prune_trims_one_incremental_batch_then_skips_empty_pass() {
    let clock = manual_clock();
    let directory = tempfile::tempdir().expect("automation fixture directory");
    let path = directory.path().join("automation.sqlite");
    let store = AutomationStore::open(&path)
        .await
        .expect("automation store should open");
    let mut observer = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
    )
    .await
    .expect("WAL observer should open");
    let now_ms = clock.now().unix_millis();
    let expired_at_ms = now_ms - 120_i64 * 24 * 60 * 60 * 1000;
    let old_payload = format!("{{\"payload\":\"{}\"}}", "x".repeat(8 * 1024));

    let mut transaction = observer
        .begin()
        .await
        .expect("fixture transaction should begin");
    for event_index in 0..512 {
        sqlx::query(
                "INSERT INTO automation_events(event_id,subject_kind,subject_id,event_kind,event_body_json,recorded_at_ms) VALUES (?,'fixture','owner','fixture',?,?)",
            )
            .bind(format!("expired-event-{event_index}"))
            .bind(&old_payload)
            .bind(expired_at_ms)
            .execute(&mut *transaction)
            .await
            .expect("expired event fixture should insert");
    }
    sqlx::query(
            "INSERT INTO automation_events(event_id,subject_kind,subject_id,event_kind,event_body_json,recorded_at_ms) VALUES ('retained-event','fixture','owner','fixture','{}',?)",
        )
        .bind(now_ms)
        .execute(&mut *transaction)
        .await
        .expect("retained event fixture should insert");
    transaction
        .commit()
        .await
        .expect("fixture transaction should commit");

    let before_pages: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(&mut observer)
        .await
        .expect("page count should be readable before pruning");
    let before_freelist: i64 = sqlx::query_scalar("PRAGMA freelist_count")
        .fetch_one(&mut observer)
        .await
        .expect("freelist should be readable before pruning");
    assert_eq!(before_freelist, 0);

    let shared_store = Arc::new(Mutex::new(store));
    let removed = AutomationRetentionWorker::new(Arc::clone(&shared_store), clock.clone())
        .prune_batch(now_ms)
        .await
        .expect("automation prune batch should complete");
    let after_pages: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(&mut observer)
        .await
        .expect("page count should be readable after pruning");
    let after_freelist: i64 = sqlx::query_scalar("PRAGMA freelist_count")
        .fetch_one(&mut observer)
        .await
        .expect("freelist should be readable after pruning");
    let retained_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM automation_events WHERE event_id='retained-event'",
    )
    .fetch_one(&mut observer)
    .await
    .expect("retained event should be readable");

    assert_eq!(removed, 512);
    assert_eq!(retained_count, 1);
    assert!((1..=64).contains(&(before_pages - after_pages)));
    assert!(after_freelist > 0);

    assert_eq!(
        AutomationRetentionWorker::new(Arc::clone(&shared_store), clock.clone())
            .prune_batch(now_ms)
            .await
            .expect("empty automation prune batch should complete"),
        0
    );
    let after_empty_pass_pages: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(&mut observer)
        .await
        .expect("page count should be readable after empty pass");
    assert_eq!(after_empty_pass_pages, after_pages);

    observer.close().await.expect("WAL observer should close");
    Arc::try_unwrap(shared_store)
        .unwrap_or_else(|_| panic!("retention worker should release automation store"))
        .into_inner()
        .close()
        .await
        .expect("automation store should close");
}
