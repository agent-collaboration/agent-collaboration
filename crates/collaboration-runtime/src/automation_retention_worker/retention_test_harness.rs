//! Automation retention on a real automation store, driven only by its injected manual clock.
//! The clock, loop and log helpers are the interaction retention worker's.
use super::AutomationRetentionWorker;
pub(super) use crate::interaction_retention_worker::retention_test_harness::{
    RetentionLog, RunningWorker, armed, at, manual_clock_at, prunes, run_to_the_armed_time,
};
use automation_storage::AutomationStore;
use chrono::{DateTime, Utc};
use sqlx::Connection;
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;

/// Starts the automation retention worker at `wall` and returns once its first pass has gone
/// to sleep.
pub(super) async fn start(
    store: &Arc<Mutex<AutomationStore>>,
    wall: DateTime<Utc>,
) -> RunningWorker {
    let clock = manual_clock_at(wall);
    let (worker_loop, pass) = AutomationRetentionWorker::new(Arc::clone(store), clock.clone())
        .into_loop()
        .await;
    RunningWorker::spawn(worker_loop, pass, clock).await
}

pub(super) async fn open_store(directory: &Path) -> Arc<Mutex<AutomationStore>> {
    Arc::new(Mutex::new(
        AutomationStore::open(&directory.join("automation.sqlite"))
            .await
            .unwrap(),
    ))
}

/// Writes fixture rows beside the store before the worker starts, as an earlier process did.
pub(super) async fn fixture_writer(directory: &Path) -> sqlx::SqliteConnection {
    sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(directory.join("automation.sqlite"))
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            // Fixture rows stand alone, without the parents an earlier process would have had.
            .foreign_keys(false),
    )
    .await
    .unwrap()
}

pub(super) async fn insert_event(
    writer: &mut sqlx::SqliteConnection,
    id: &str,
    stamped: DateTime<Utc>,
) {
    sqlx::query("INSERT INTO automation_events(event_id,subject_kind,subject_id,event_kind,event_body_json,recorded_at_ms) VALUES (?,'fixture','owner','fixture','{}',?)")
        .bind(id)
        .bind(stamped.timestamp_millis())
        .execute(writer)
        .await
        .unwrap();
}

pub(super) async fn revisions(writer: &mut sqlx::SqliteConnection) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM instruction_revisions")
        .fetch_one(writer)
        .await
        .unwrap()
}
