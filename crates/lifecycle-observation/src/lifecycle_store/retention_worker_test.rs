//! Lifecycle retention on a real journal: it arms the first moment expiry may remove a
//! record or a checkpointed address, expires only then, sleeps through a commit that
//! leaves nothing due, and retries a failed expiry on its backoff.
use super::{RetentionObservation, observed::RetentionSnapshot};
use crate::{JournalError, LifecycleStore, ObservationJournal};
use chrono::{DateTime, Utc};
use collaboration_protocol::LifecycleObservation;
use runtime_primitives::{Clock, SyncTime};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex},
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::{Layer, layer::SubscriberExt};

#[tokio::test]
async fn record_expiry_reads_and_waits_only_on_the_injected_clock() {
    use router_clock::{ClockInstant, ManualClock};
    use std::time::Duration;

    let journal = Journal::open().await;
    journal
        .append(&backend_observation(), seconds("2001-01-15T10:00:00Z"))
        .await;
    let clock = Arc::new(ManualClock::new(at("2001-03-15T10:00:00Z")));
    let observation = Arc::new(RetentionObservation::default());
    let shutdown = CancellationToken::new();
    let task = tokio::spawn({
        let store = journal.store.clone();
        let clock = clock.clone();
        let observation = observation.clone();
        let shutdown = shutdown.clone();
        async move { store.run_retention(&*clock, &shutdown, &observation).await }
    });
    observation.observed.asleep_at_least(1).await;
    let deadline = ClockInstant::ZERO.after(Duration::from_secs(1));
    assert!(
        clock.has_not_due_check_now(deadline),
        "retention must register its injected sleep"
    );
    assert_eq!(journal.count("lifecycle_records").await, 1);

    clock.advance(Duration::from_millis(999)).unwrap();
    clock.not_due_check_now(deadline).await;
    assert_eq!(journal.count("lifecycle_records").await, 1);
    clock.advance(Duration::from_millis(1)).unwrap();
    observation.observed.asleep_at_least(2).await;

    assert_eq!(journal.count("lifecycle_records").await, 0);
    assert_eq!(
        observation.observed.snapshot().last_expiry_entered_at,
        Some(SyncTime::from_unix_millis(
            at("2001-03-15T10:00:01Z").timestamp_millis()
        ))
    );
    shutdown.cancel();
    task.await.unwrap();
}

use router_clock::ManualClock;

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

fn seconds(text: &str) -> i64 {
    at(text).timestamp()
}

const ENDPOINT: &str =
    r#"{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"}"#;

/// An observation with no thread, so expiry checkpoints no address for it.
fn backend_observation() -> LifecycleObservation {
    let endpoint: serde_json::Value = serde_json::from_str(ENDPOINT).unwrap();
    serde_json::from_value(serde_json::json!({
        "observedAt": "2026-01-01T00:00:00Z",
        "source": "observerLifecycle",
        "scope": {"endpoint": endpoint, "generation": null, "observerId": "00000000-0000-4000-8000-000000000002"},
        "subject": {"kind": "backend"},
        "change": {"kind": "coverageLost"}
    }))
    .unwrap()
}

/// A thread observation; expiring its record checkpoints the thread's address, last seen
/// at `observed_at`.
fn thread_observation(observed_at: &str) -> LifecycleObservation {
    let endpoint: serde_json::Value = serde_json::from_str(ENDPOINT).unwrap();
    serde_json::from_value(serde_json::json!({
        "observedAt": observed_at,
        "source": "inventoryRead",
        "scope": {"endpoint": endpoint, "generation": null, "observerId": "00000000-0000-4000-8000-000000000002"},
        "subject": {"kind": "thread", "address": {"endpoint": endpoint, "nativeThreadId": "retained-thread"}},
        "change": {"kind": "threadDiscovered"}
    }))
    .unwrap()
}

struct Journal {
    _directory: tempfile::TempDir,
    path: PathBuf,
    store: Arc<LifecycleStore>,
}

impl Journal {
    async fn open() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session-registry.sqlite");
        let journal = ObservationJournal::open(
            &path,
            "00000000-0000-4000-8000-000000000001"
                .to_owned()
                .try_into()
                .unwrap(),
        )
        .await
        .unwrap();
        Self {
            _directory: directory,
            path,
            store: Arc::new(LifecycleStore::new(journal)),
        }
    }

    async fn append(&self, observation: &LifecycleObservation, at_seconds: i64) {
        self.store.append(observation, at_seconds).await.unwrap();
    }

    /// Counts rows through a connection of its own, which the store never signals.
    async fn count(&self, table: &str) -> i64 {
        let query = match table {
            "lifecycle_records" => "SELECT COUNT(*) FROM lifecycle_records",
            "checkpoint_addresses" => "SELECT COUNT(*) FROM checkpoint_addresses",
            "thread_addresses" => "SELECT COUNT(*) FROM thread_addresses",
            other => panic!("no count for {other}"),
        };
        let mut connection = fixture_connection(&self.path).await;
        let count = sqlx::query_scalar(query)
            .fetch_one(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
        count
    }

    async fn close(self, worker: RunningRetention) {
        worker.stop().await;
        Arc::try_unwrap(self.store)
            .unwrap_or_else(|_| panic!("the worker released the store"))
            .close()
            .await;
    }
}

async fn fixture_connection(path: &Path) -> SqliteConnection {
    SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(path))
        .await
        .unwrap()
}

struct RunningRetention {
    observation: Arc<RetentionObservation>,
    clock: Arc<ManualClock>,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl RunningRetention {
    /// Starts the loop at `wall` and returns once its first pass has gone to sleep.
    async fn start(journal: &Journal, wall: &str) -> Self {
        let clock = Arc::new(ManualClock::new(at(wall)));
        let observation = Arc::new(RetentionObservation::default());
        let shutdown = CancellationToken::new();
        let task = tokio::spawn({
            let store = journal.store.clone();
            let observation = observation.clone();
            let shutdown = shutdown.clone();
            let clock = clock.clone();
            async move { store.run_retention(&*clock, &shutdown, &observation).await }
        });
        observation.observed.asleep_at_least(1).await;
        Self {
            observation,
            clock,
            shutdown,
            task,
        }
    }

    fn snapshot(&self) -> RetentionSnapshot {
        self.observation.observed.snapshot()
    }

    fn armed(&self) -> Option<DateTime<Utc>> {
        self.snapshot()
            .armed
            .and_then(|deadline| DateTime::from_timestamp_millis(deadline.unix_millis()))
    }

    /// The pass must enter at the exact deadline the manual clock reached.
    fn assert_last_expiry_entered_at(&self, due: DateTime<Utc>) {
        let entered = self.snapshot().last_expiry_entered_at.unwrap();
        let late = entered.unix_millis() - due.timestamp_millis();
        assert_eq!(late, 0, "the pass began {late} ms after {due}");
    }

    /// Advances the manual clock to the armed deadline and awaits the resulting pass.
    async fn run_to_the_armed_time(&self) {
        let sleeps = self.snapshot().sleeps;
        self.clock
            .advance_to(self.snapshot().armed.unwrap())
            .unwrap();
        self.observation.observed.asleep_at_least(sleeps + 1).await;
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }
}

#[tokio::test]
async fn a_record_is_armed_at_the_first_whole_second_the_rule_allows_and_expired_only_then() {
    for (stamped, first_expiry) in [
        // Mid-month: the first whole second after exactly two months.
        ("2026-01-15T10:00:00Z", "2026-03-15T10:00:01Z"),
        // Month end: the cutoff from late February clamps to 2025-12-28, so the record
        // waits for March.
        ("2025-12-31T12:00:00Z", "2026-03-01T00:00:00Z"),
    ] {
        let journal = Journal::open().await;
        journal
            .append(&backend_observation(), seconds(stamped))
            .await;
        let worker = RunningRetention::start(&journal, "2026-02-01T00:00:00Z").await;

        assert_eq!(worker.armed(), Some(at(first_expiry)), "{stamped}");
        assert_eq!(
            worker.snapshot().expiries,
            0,
            "nothing expires early: {stamped}"
        );
        assert_eq!(journal.count("lifecycle_records").await, 1);

        worker.run_to_the_armed_time().await;

        worker.assert_last_expiry_entered_at(at(first_expiry));
        assert_eq!(journal.count("lifecycle_records").await, 0, "{stamped}");
        assert_eq!(
            worker.snapshot().expiries,
            1,
            "one expiry at the due time: {stamped}"
        );
        assert_eq!(worker.armed(), None, "nothing is left to arm: {stamped}");
        journal.close(worker).await;
    }
}

#[tokio::test]
async fn a_checkpointed_address_is_armed_from_its_last_millisecond_and_pruned_then() {
    let journal = Journal::open().await;
    journal
        .append(
            &thread_observation("2026-03-10T00:00:00.500Z"),
            seconds("2026-01-01T00:00:00Z"),
        )
        .await;
    let worker = RunningRetention::start(&journal, "2026-03-05T00:00:00Z").await;

    // The record was due on start; expiring it checkpointed the address, which a
    // whole-second cutoff first passes one second after the address was last seen.
    assert_eq!(journal.count("lifecycle_records").await, 0);
    assert_eq!(journal.count("checkpoint_addresses").await, 1);
    assert_eq!(worker.armed(), Some(at("2026-05-10T00:00:01Z")));
    assert_eq!(worker.snapshot().expiries, 1);

    worker.run_to_the_armed_time().await;

    worker.assert_last_expiry_entered_at(at("2026-05-10T00:00:01Z"));
    assert_eq!(journal.count("checkpoint_addresses").await, 0);
    assert_eq!(journal.count("thread_addresses").await, 0);
    assert_eq!(worker.snapshot().expiries, 2);
    assert_eq!(worker.armed(), None);
    journal.close(worker).await;
}

#[tokio::test]
async fn a_stored_retention_clock_ahead_of_the_wall_clock_expires_by_that_clock() {
    let journal = Journal::open().await;
    journal
        .append(&backend_observation(), seconds("2026-01-01T00:00:00Z"))
        .await;
    // A later append moved the retention clock past the wall clock the loop starts at.
    journal
        .append(&backend_observation(), seconds("2026-04-01T00:00:00Z"))
        .await;
    let worker = RunningRetention::start(&journal, "2026-02-15T00:00:00Z").await;

    assert_eq!(
        journal.count("lifecycle_records").await,
        1,
        "the old record went at once"
    );
    assert_eq!(worker.snapshot().expiries, 1);
    assert_eq!(worker.armed(), Some(at("2026-06-01T00:00:01Z")));
    journal.close(worker).await;
}

#[tokio::test]
async fn a_commit_that_leaves_nothing_due_reads_the_due_time_and_expires_nothing() {
    let journal = Journal::open().await;
    journal
        .append(&backend_observation(), seconds("2026-01-15T10:00:00Z"))
        .await;
    let worker = RunningRetention::start(&journal, "2026-02-01T00:00:00Z").await;
    let before = worker.snapshot();

    journal
        .append(&backend_observation(), seconds("2026-02-01T00:00:00Z"))
        .await;
    worker
        .observation
        .observed
        .asleep_at_least(before.sleeps + 1)
        .await;

    let after = worker.snapshot();
    assert!(after.due_reads > before.due_reads, "the commit was read");
    assert_eq!(after.expiries, 0, "nothing was due, so nothing was expired");
    assert_eq!(after.armed, before.armed, "the due time is unchanged");
    assert_eq!(journal.count("lifecycle_records").await, 2);
    journal.close(worker).await;
}

/// Counts the loop's failure warnings.
struct FailureLog(Arc<StdMutex<usize>>);
impl<TSubscriber: tracing::Subscriber> Layer<TSubscriber> for FailureLog {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, TSubscriber>,
    ) {
        #[derive(Default)]
        struct Message(Option<String>);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = Some(format!("{value:?}"));
                }
            }
        }
        let mut message = Message::default();
        event.record(&mut message);
        if message.0.as_deref() == Some("lifecycle_observation.maintenance_degraded") {
            *self.0.lock().unwrap() += 1;
        }
    }
}

/// Adds or removes an expiry fault through a connection of its own, which the store's
/// commit signal never sees.
async fn set_expiry_fault(path: &Path, present: bool) {
    let mut connection = fixture_connection(path).await;
    let statement = if present {
        "CREATE TRIGGER fail_lifecycle_expiry BEFORE DELETE ON lifecycle_records BEGIN SELECT RAISE(FAIL, 'lifecycle expiry failure'); END"
    } else {
        "DROP TRIGGER fail_lifecycle_expiry"
    };
    sqlx::raw_sql(statement)
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn a_failed_expiry_retries_at_one_two_four_seconds_and_recovers_with_no_new_write() {
    let failures = Arc::new(StdMutex::new(0_usize));
    let _trace = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(FailureLog(failures.clone())),
    );
    let journal = Journal::open().await;
    journal
        .append(&backend_observation(), seconds("2026-01-01T00:00:00Z"))
        .await;
    set_expiry_fault(&journal.path, true).await;
    let wall = at("2026-06-15T12:00:00Z");

    let worker = RunningRetention::start(&journal, "2026-06-15T12:00:00Z").await;

    let mut previous = worker.armed().unwrap();
    let parked_wall =
        DateTime::<Utc>::from_timestamp_millis(worker.clock.now().unix_millis()).unwrap();
    // The failed pass ran between the start wall time and the moment the loop parked.
    assert!(
        previous >= wall + chrono::Duration::seconds(1)
            && previous <= parked_wall + chrono::Duration::seconds(1),
        "the first retry is one second after the failure: {previous}"
    );
    for expected in [2_000, 4_000] {
        worker.run_to_the_armed_time().await;
        let next = worker.armed().unwrap();
        let step = (next - previous).num_milliseconds();
        // The manual clock reaches each retry's exact deadline.
        assert_eq!(step, expected, "retry delay must match its exact deadline");
        previous = next;
    }
    assert_eq!(*failures.lock().unwrap(), 1, "one warning for the run");
    assert_eq!(journal.count("lifecycle_records").await, 1);

    set_expiry_fault(&journal.path, false).await;
    let retry_at = worker.armed().unwrap();
    worker.run_to_the_armed_time().await;

    worker.assert_last_expiry_entered_at(retry_at);
    assert_eq!(
        journal.count("lifecycle_records").await,
        0,
        "the retry needed only its time"
    );
    assert_eq!(worker.armed(), None);
    // A failed expiry marked the journal unavailable, and retention doesn't restore it.
    assert!(matches!(
        journal.store.bounds().await,
        Err(JournalError::InvalidStorage)
    ));
    journal.close(worker).await;
}
