//! Retention read/delete failures back off independently while another class succeeds,
//! log one warning for the run, and recover with no new owner mutation.
use super::interaction_history_retention_tests::{history_value, load_broker};
use super::retention_test_harness::*;
use chrono::{DateTime, SecondsFormat, Utc};
use runtime_primitives::Clock;
use sqlx::Connection;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex as StdMutex},
};
use tracing_subscriber::layer::SubscriberExt;

#[derive(Clone, Copy, Debug)]
enum FaultedClass {
    Records,
    History,
    HistoryRead,
}

impl FaultedClass {
    /// The name the worker logs the class under.
    const fn logged_name(self) -> &'static str {
        match self {
            Self::Records => "interaction records",
            Self::History | Self::HistoryRead => "interaction history",
        }
    }

    /// The table whose DELETE the fault makes fail.
    const fn table(self) -> &'static str {
        match self {
            Self::Records => "approval_routes",
            Self::History | Self::HistoryRead => "typed_interaction_history",
        }
    }

    /// The aged row of this class, counted by query.
    const fn remaining_query(self) -> &'static str {
        match self {
            Self::Records => "SELECT COUNT(*) FROM approval_routes WHERE thread_id='faulted-route'",
            Self::History | Self::HistoryRead => {
                "SELECT COUNT(*) FROM typed_interaction_history WHERE request_id='faulted-history'"
            }
        }
    }

    /// The other class, whose aged row goes in the same passes.
    const fn other(self) -> Self {
        match self {
            Self::Records => Self::History,
            Self::History | Self::HistoryRead => Self::Records,
        }
    }
}

const AGED: &str = "2026-01-01T00:00:00.000Z";
const WALL: &str = "2026-06-15T12:00:00.000Z";

async fn interaction_connection(directory: &Path) -> sqlx_turso::TursoConnection {
    sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new().filename(directory.join("interaction.sqlite")),
    )
    .await
    .unwrap()
}

/// Writes an aged history row before the broker loads, as an earlier process did.
async fn seed_aged_history(directory: &Path) {
    let mut history = interaction_connection(directory).await;
    let record = history_value("faulted-history", "aged refusal", None).unwrap();
    let mut transaction = history.begin().await.unwrap();
    sqlx::query("INSERT INTO typed_interaction_history (request_id,record_json,created_at) VALUES ('faulted-history',?,?)")
        .bind(serde_json::to_string(&record).unwrap())
        .bind(at(AGED).to_rfc3339_opts(SecondsFormat::Nanos, true))
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("UPDATE interaction_history_revision SET revision=revision+1")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    history.close().await.unwrap();
}

async fn remaining_rows(class: FaultedClass, directory: &Path) -> i64 {
    let mut connection = interaction_connection(directory).await;
    let remaining = sqlx::query_scalar::<sqlx_turso::Turso, i64>(class.remaining_query())
        .fetch_one(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    remaining
}

async fn set_fault(class: FaultedClass, directory: &Path, present: bool) {
    let statement = if matches!(class, FaultedClass::HistoryRead) {
        if present {
            "ALTER TABLE typed_interaction_history RENAME COLUMN record_json TO unavailable_record_json"
        } else {
            "ALTER TABLE typed_interaction_history RENAME COLUMN unavailable_record_json TO record_json"
        }
        .to_owned()
    } else if present {
        format!(
            "CREATE TRIGGER fail_class_prune BEFORE DELETE ON {} BEGIN SELECT RAISE(FAIL, 'class prune failure'); END",
            class.table()
        )
    } else {
        "DROP TRIGGER fail_class_prune".to_owned()
    };
    let mut connection = interaction_connection(directory).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
}

async fn assert_class_backs_off_and_recovers(class: FaultedClass) {
    let counts = Arc::new(StdMutex::new(HashMap::new()));
    let _trace = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(RetentionLog(counts.clone())),
    );
    let directory = tempfile::tempdir().unwrap();
    let gate = crate::NativeGenerationGate::default();
    // Loading the broker once creates the history tables the fixture writes.
    drop(load_broker(directory.path(), &gate).await.unwrap());
    seed_aged_history(directory.path()).await;
    let broker = load_broker(directory.path(), &gate).await.unwrap();
    let interactions = broker.interaction_store();
    insert_route(&interactions, "faulted-route", at(AGED)).await;
    set_fault(class, directory.path(), true).await;
    let wall = at(WALL);

    let worker = RunningWorker::start(Some(interactions), Some(broker), wall).await;

    let context = format!("{class:?}");
    assert_eq!(
        remaining_rows(class.other(), directory.path()).await,
        0,
        "the other class went on: {context}"
    );
    assert_eq!(
        remaining_rows(class, directory.path()).await,
        1,
        "{context}"
    );
    let first_retry = armed(&worker.snapshot()).unwrap();
    let parked_wall =
        DateTime::<Utc>::from_timestamp_millis(worker.clock.now().unix_millis()).unwrap();
    assert!(
        first_retry >= wall + chrono::Duration::seconds(1)
            && first_retry <= parked_wall + chrono::Duration::seconds(1),
        "the first retry is one second after the failure: {context} {first_retry}"
    );
    let mut previous = first_retry;
    for expected in [2_000, 4_000] {
        run_to_the_armed_time(&worker).await;
        let next = armed(&worker.snapshot()).unwrap();
        let step = (next - previous).num_milliseconds();
        // The manual clock reaches each retry's exact deadline.
        assert!(
            (expected..=expected + 2).contains(&step),
            "{context}: step {step}, expected {expected}"
        );
        previous = next;
    }
    let logged = |message: &str| {
        counts
            .lock()
            .unwrap()
            .get(&format!("{}: {message}", class.logged_name()))
            .copied()
    };
    assert_eq!(
        logged("retention prune failed; it retries with backoff"),
        Some(1),
        "{context}"
    );

    set_fault(class, directory.path(), false).await;
    run_to_the_armed_time(&worker).await;

    assert_eq!(
        remaining_rows(class, directory.path()).await,
        0,
        "the retry needed only its time: {context}"
    );
    assert_eq!(logged("retention prune recovered"), Some(1), "{context}");
    worker.stop().await;
}

#[tokio::test]
async fn a_history_inventory_read_failure_backs_off_without_holding_other_retention_classes() {
    assert_class_backs_off_and_recovers(FaultedClass::HistoryRead).await;
}

#[tokio::test]
async fn every_retention_class_backs_off_on_its_own_and_recovers_after_its_delete_fails() {
    for class in [FaultedClass::Records, FaultedClass::History] {
        assert_class_backs_off_and_recovers(class).await;
    }
}
