//! Provider operation retention on a real store and supervisor: it arms the first moment
//! the rule lets a settled operation go and deletes only then, keeps an operation the
//! supervisor still holds live until it publishes its result with no store write, reads a
//! commit that leaves nothing due without deleting, and retries a failed prune.
use super::provider_retention_loop;
use crate::ExternalProviderSupervisor;
use chrono::{DateTime, Utc};
use collaboration_protocol::{
    CodexGeneration, ConversationBindingIdentity, EndpointId, EndpointRef, GenerationNumber,
    NonEmptyText, OperationId, ProviderBindingId, ProviderBindingIdentity, ProviderCapabilities,
    ProviderCapability, ProviderCapabilityEvidence, ProviderCapabilityName,
    ProviderCapabilityStatus, ProviderKind, ProviderOperationEffect, ProviderOperationKind,
    ProviderReconciliationState, ProviderRuntimeIdentity, ProviderTransport, UuidIdentity,
};
use collaboration_runtime::{
    LoopSnapshot, ProviderOperationAdmission, ProviderOperationStore, WorkerObservation,
    paused_wall_clock::PausedWallClock,
};
use runtime_primitives::{Clock, SyncTime};
use sqlx::Connection;
use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex},
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::{Layer, layer::SubscriberExt};

const WORKER: &str = "provider_operation_retention";

/// Bounds, in real time, a wait for a pass the test expects; it only stops a hang.
const WAKE_GUARD: std::time::Duration = std::time::Duration::from_secs(30);

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

fn binding() -> ProviderBindingIdentity {
    let endpoint = EndpointRef {
        service_id: UuidIdentity::try_from("0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89".to_owned())
            .unwrap(),
        endpoint_id: EndpointId::try_from("cursor-local".to_owned()).unwrap(),
    };
    ProviderBindingIdentity {
        endpoint,
        binding_id: ProviderBindingId::try_from("cursor-binding".to_owned()).unwrap(),
        runtime: ProviderRuntimeIdentity {
            provider: ProviderKind::Cursor,
            runtime_name: NonEmptyText::try_from("cursor".to_owned()).unwrap(),
            runtime_version: None,
        },
        transport: ProviderTransport::StdioAcp,
        generation: CodexGeneration {
            service_epoch: UuidIdentity::try_from(
                "1ff962c5-7fa3-4c18-a5ca-1bbe8db09e80".to_owned(),
            )
            .unwrap(),
            generation: GenerationNumber::try_from(1).unwrap(),
        },
        capabilities: ProviderCapabilities::try_from(vec![ProviderCapability {
            name: ProviderCapabilityName::Create,
            status: ProviderCapabilityStatus::Supported,
            evidence: ProviderCapabilityEvidence::Advertised,
        }])
        .unwrap(),
    }
}

struct Operations {
    _directory: tempfile::TempDir,
    path: PathBuf,
    store: Arc<Mutex<ProviderOperationStore>>,
    supervisor: Arc<ExternalProviderSupervisor>,
}

impl Operations {
    async fn open() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("provider-operations.sqlite");
        let store = Arc::new(Mutex::new(
            ProviderOperationStore::open(&path).await.unwrap(),
        ));
        let supervisor =
            Arc::new(ExternalProviderSupervisor::new(Vec::new(), Arc::clone(&store)).unwrap());
        Self {
            _directory: directory,
            path,
            store,
            supervisor,
        }
    }

    /// Admits an operation and marks it dispatched, as running it does.
    async fn dispatched(&self) -> OperationId {
        let mut store = self.store.lock().await;
        let operation_id = OperationId::generate();
        store
            .admit(ProviderOperationAdmission {
                operation_id: operation_id.clone(),
                operation_kind: ProviderOperationKind::ConversationPrompt,
                binding: ConversationBindingIdentity::ExternalProvider { binding: binding() },
                admitted_at_ms: 1,
            })
            .await
            .unwrap();
        store
            .mark_may_have_dispatched(&operation_id, 2)
            .await
            .unwrap();
        operation_id
    }

    /// Records the operation's settlement, as the supervisor does before it publishes.
    async fn settle(&self, operation_id: &OperationId, settled: DateTime<Utc>) {
        self.store
            .lock()
            .await
            .record_terminal(
                operation_id,
                ProviderOperationEffect::Applied,
                ProviderReconciliationState::Confirmed,
                None,
                settled.timestamp_millis(),
            )
            .await
            .unwrap();
    }

    async fn settled(&self, settled: DateTime<Utc>) -> OperationId {
        let operation_id = self.dispatched().await;
        self.settle(&operation_id, settled).await;
        operation_id
    }

    async fn holds(&self, operation_id: &OperationId) -> bool {
        self.store
            .lock()
            .await
            .inspect(operation_id)
            .await
            .unwrap()
            .is_some()
    }
}

struct RunningRetention {
    observation: WorkerObservation,
    clock: Arc<PausedWallClock>,
    prunes_entered_at: Arc<StdMutex<Vec<SyncTime>>>,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl RunningRetention {
    /// Starts the worker at `wall` and returns once its first pass has gone to sleep.
    async fn start(
        operations: &Operations,
        retention_days: Option<NonZeroU32>,
        wall: DateTime<Utc>,
    ) -> Self {
        let clock = Arc::new(PausedWallClock::starting_at(SyncTime::from_unix_millis(
            wall.timestamp_millis(),
        )));
        let (worker_loop, mut pass) = provider_retention_loop(
            Arc::clone(&operations.store),
            Some(Arc::clone(&operations.supervisor)),
            retention_days,
            clock.clone(),
        )
        .await;
        let prunes_entered_at = pass.prunes_entered_at.clone();
        let observation = worker_loop.observation();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn({
            let shutdown = shutdown.clone();
            async move { worker_loop.run(&mut pass, shutdown).await }
        });
        tokio::time::timeout(WAKE_GUARD, observation.asleep_at_least(1))
            .await
            .expect("the first pass goes to sleep");
        Self {
            observation,
            clock,
            prunes_entered_at,
            shutdown,
            task,
        }
    }

    fn snapshot(&self) -> LoopSnapshot {
        self.observation.snapshot()
    }

    fn armed(&self) -> Option<DateTime<Utc>> {
        self.snapshot()
            .armed_deadline
            .and_then(|deadline| DateTime::from_timestamp_millis(deadline.unix_millis()))
    }

    fn reads(&self, read: &str) -> usize {
        self.snapshot()
            .reads
            .iter()
            .filter(|seen| **seen == read)
            .count()
    }

    fn prunes(&self) -> usize {
        self.reads("provider_operation_retention.prune")
    }

    /// Returns once the worker has slept after the pass a change it was just given wakes.
    async fn asleep_after(&self, sleeps: u64, cause: &str) {
        tokio::time::timeout(WAKE_GUARD, self.observation.asleep_at_least(sleeps + 1))
            .await
            .unwrap_or_else(|_| panic!("{cause} wakes the worker"));
    }

    /// Asserts the latest prune began at `due`, as the pass read its clock on waking, up
    /// to the paused timer's 2 ms rounding: a timer that fires late fails this.
    fn assert_last_prune_entered_at(&self, due: DateTime<Utc>) {
        let entered = *self.prunes_entered_at.lock().unwrap().last().unwrap();
        let late = entered.unix_millis() - due.timestamp_millis();
        assert!(
            (0..=2).contains(&late),
            "the prune began {late} ms after {due}"
        );
    }

    /// Lets the paused clock reach the worker's timer and returns once that pass has slept.
    async fn run_to_the_armed_time(&self) {
        let sleeps = self.snapshot().sleeps;
        tokio::time::pause();
        self.observation.asleep_at_least(sleeps + 1).await;
        tokio::time::resume();
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }
}

#[tokio::test]
async fn the_worker_arms_the_first_moment_the_rule_allows_and_deletes_only_then() {
    for (retention_days, settled, first_deletable) in [
        // Mid-month: one millisecond after exactly two months.
        (None, "2026-01-15T10:00:00.000Z", "2026-03-15T10:00:00.001Z"),
        // Month end: the cutoff from late February clamps to 2025-12-28, so the operation
        // waits for March.
        (None, "2025-12-31T12:00:00.000Z", "2026-03-01T00:00:00.000Z"),
        // A day override: one millisecond after exactly that many days.
        (
            NonZeroU32::new(10),
            "2026-01-25T10:00:00.000Z",
            "2026-02-04T10:00:00.001Z",
        ),
    ] {
        let operations = Operations::open().await;
        let operation = operations.settled(at(settled)).await;
        let worker =
            RunningRetention::start(&operations, retention_days, at("2026-02-01T00:00:00.000Z"))
                .await;

        assert_eq!(worker.armed(), Some(at(first_deletable)), "{settled}");
        assert_eq!(worker.prunes(), 0, "nothing is deleted before it is due");
        assert!(operations.holds(&operation).await);

        worker.run_to_the_armed_time().await;

        worker.assert_last_prune_entered_at(at(first_deletable));
        assert!(!operations.holds(&operation).await, "{settled}");
        assert_eq!(worker.prunes(), 1, "one DELETE, at the due time: {settled}");
        assert_eq!(worker.armed(), None, "nothing is left to arm: {settled}");
        worker.stop().await;
    }
}

#[tokio::test]
async fn a_live_operation_is_kept_until_it_publishes_and_then_goes_at_its_due_time() {
    let operations = Operations::open().await;
    let operation = operations.dispatched().await;
    operations.supervisor.hold_live_operation(operation.clone());
    let worker = RunningRetention::start(&operations, None, at("2026-02-01T00:00:00.000Z")).await;

    // The supervisor records the settlement before it publishes the result.
    let sleeps = worker.snapshot().sleeps;
    operations
        .settle(&operation, at("2026-01-15T10:00:00.000Z"))
        .await;
    worker.asleep_after(sleeps, "the settlement").await;
    assert_eq!(worker.armed(), None, "a live operation arms no timer");

    let sleeps = worker.snapshot().sleeps;
    assert!(operations.supervisor.publish_held_result(&operation).await);
    worker.asleep_after(sleeps, "publishing the result").await;
    assert_eq!(worker.armed(), Some(at("2026-03-15T10:00:00.001Z")));
    assert_eq!(worker.prunes(), 0, "not yet due");

    worker.run_to_the_armed_time().await;

    worker.assert_last_prune_entered_at(at("2026-03-15T10:00:00.001Z"));
    assert!(!operations.holds(&operation).await);
    worker.stop().await;
}

#[tokio::test]
async fn a_live_operation_past_its_due_time_goes_as_soon_as_it_publishes() {
    let operations = Operations::open().await;
    let operation = operations.dispatched().await;
    operations.supervisor.hold_live_operation(operation.clone());
    operations
        .settle(&operation, at("2026-01-01T00:00:00.000Z"))
        .await;
    let worker = RunningRetention::start(&operations, None, at("2026-06-15T12:00:00.000Z")).await;
    assert_eq!(worker.armed(), None, "a live operation arms no timer");
    assert_eq!(worker.prunes(), 0);

    let sleeps = worker.snapshot().sleeps;
    assert!(operations.supervisor.publish_held_result(&operation).await);
    worker.asleep_after(sleeps, "publishing the result").await;

    assert!(!operations.holds(&operation).await);
    assert_eq!(worker.prunes(), 1);
    assert_eq!(worker.armed(), None);
    worker.stop().await;
}

#[tokio::test]
async fn a_commit_that_leaves_nothing_due_reads_the_due_time_and_deletes_nothing() {
    let operations = Operations::open().await;
    let older = operations.settled(at("2026-01-15T10:00:00.000Z")).await;
    // Dispatched before the worker parks, so the settlement below is the only commit after.
    let newer = operations.dispatched().await;
    let worker = RunningRetention::start(&operations, None, at("2026-02-01T00:00:00.000Z")).await;
    let before = worker.snapshot();
    let due_reads_before = worker.reads("provider_operation_retention.due");

    operations
        .settle(&newer, at("2026-01-20T00:00:00.000Z"))
        .await;
    worker.asleep_after(before.sleeps, "the settlement").await;

    assert!(
        worker.reads("provider_operation_retention.due") > due_reads_before,
        "the commit was read"
    );
    assert_eq!(
        worker.prunes(),
        0,
        "nothing was due, so nothing was deleted"
    );
    assert_eq!(worker.snapshot().armed_deadline, before.armed_deadline);
    assert!(operations.holds(&older).await);
    assert!(operations.holds(&newer).await);
    worker.stop().await;
}

/// Counts the loop's failure warnings for this worker.
struct FailureLog(Arc<StdMutex<usize>>);
impl<TSubscriber: tracing::Subscriber> Layer<TSubscriber> for FailureLog {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, TSubscriber>,
    ) {
        #[derive(Default)]
        struct Fields {
            worker: Option<String>,
            message: Option<String>,
        }
        impl tracing::field::Visit for Fields {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "worker" {
                    self.worker = Some(value.to_owned());
                }
            }
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.message = Some(format!("{value:?}"));
                }
            }
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        if fields.worker.as_deref() == Some(WORKER)
            && fields.message.as_deref() == Some("worker pass failed; it retries with backoff")
        {
            *self.0.lock().unwrap() += 1;
        }
    }
}

/// Adds or removes a DELETE fault through a connection of its own, which the store's
/// commit signal never sees.
async fn set_delete_fault(path: &Path, present: bool) {
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(path),
    )
    .await
    .unwrap();
    let statement = if present {
        "CREATE TRIGGER fail_provider_prune BEFORE DELETE ON provider_operations BEGIN SELECT RAISE(FAIL, 'provider prune failure'); END"
    } else {
        "DROP TRIGGER fail_provider_prune"
    };
    sqlx::raw_sql(statement)
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn a_failed_prune_retries_at_one_two_four_seconds_and_recovers_with_no_new_write() {
    let failures = Arc::new(StdMutex::new(0_usize));
    let _trace = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(FailureLog(failures.clone())),
    );
    let operations = Operations::open().await;
    let operation = operations.settled(at("2026-01-01T00:00:00.000Z")).await;
    set_delete_fault(&operations.path, true).await;
    let wall = at("2026-06-15T12:00:00.000Z");

    let worker = RunningRetention::start(&operations, None, wall).await;

    let first_retry = worker.armed().unwrap();
    let parked_wall =
        DateTime::<Utc>::from_timestamp_millis(worker.clock.now().unix_millis()).unwrap();
    assert!(
        first_retry >= wall + chrono::Duration::seconds(1)
            && first_retry <= parked_wall + chrono::Duration::seconds(1),
        "the first retry is one second after the failure: {first_retry}"
    );
    let mut previous = first_retry;
    for expected in [2_000, 4_000] {
        worker.run_to_the_armed_time().await;
        let next = worker.armed().unwrap();
        let step = (next - previous).num_milliseconds();
        // The pass starts at the armed time or up to the paused timer's 2 ms after it.
        assert!(
            (expected..=expected + 2).contains(&step),
            "step {step}, expected {expected}"
        );
        previous = next;
    }
    assert_eq!(*failures.lock().unwrap(), 1, "one warning for the run");
    assert!(operations.holds(&operation).await);

    set_delete_fault(&operations.path, false).await;
    let retry_at = worker.armed().unwrap();
    worker.run_to_the_armed_time().await;

    worker.assert_last_prune_entered_at(retry_at);
    assert!(
        !operations.holds(&operation).await,
        "the retry needed only its time"
    );
    assert_eq!(worker.armed(), None);
    worker.stop().await;
}
