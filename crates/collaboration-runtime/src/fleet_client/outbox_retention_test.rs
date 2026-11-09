//! Fleet outbox retention on a real Fleet database: it arms the oldest deletable
//! settlement, deletes only once the strict cutoff passes, ignores a request its creation
//! input still keeps, and retries a failed prune on its backoff.
use super::outbox_retention_loop;
use crate::{LoopSnapshot, WorkerObservation, paused_wall_clock::PausedWallClock};
use chrono::{DateTime, Utc};
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::*;
use message_board_storage::{FleetOutbox, FleetStore};
use runtime_primitives::{Clock, SyncTime};
use sqlx::Connection;
use std::{
    path::Path,
    sync::{Arc, Mutex as StdMutex},
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::{Layer, layer::SubscriberExt};

const WORKER: &str = "fleet_outbox_retention";

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

fn creation() -> (ProjectCreateRequest, FleetRequest) {
    let input = ProjectCreateRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: ProjectId::generate(),
        board_id: BoardId::generate(),
        title: ResourceName::try_from("retained creation".to_owned()).unwrap(),
        description: None,
        actor: Identity::Human {
            human_id: HumanId::try_from("owner".to_owned()).unwrap(),
        },
        acting_for: None,
    };
    let directory = FleetRequest {
        request_id: input.request_id.clone(),
        workspace: WorkspaceId::generate(),
        actor: RecordedActor::declared(input.actor.clone()),
        acting_for: input.acting_for.clone(),
        protocol_version: input.protocol_version,
        operation: FleetOperation::ProjectDirectoryCreate {
            project: input.project_id.clone(),
            board: input.board_id.clone(),
            title: input.title.clone(),
        },
    };
    (input, directory)
}

fn accepted() -> RequestDecision {
    RequestDecision::Accepted {
        effects: DecisionEffectsPage::from_full(
            Vec::new(),
            &DecisionEffectsHeader::from_full(&[]),
            None,
        )
        .unwrap(),
        change: ChangeOutcome::Changed,
    }
}

/// A rejected request settled at `settled`; retention may delete it once it ages out.
async fn settle_rejected(outbox: &FleetOutbox, settled: DateTime<Utc>) -> RequestId {
    let (_, request) = creation();
    outbox.enqueue(&request).await.unwrap();
    outbox
        .mark_decided(
            &request.request_id,
            &RequestDecision::Rejected {
                reasons: Vec::new(),
            },
            settled.timestamp_millis(),
        )
        .await
        .unwrap();
    request.request_id
}

/// An accepted creation settled at `settled` whose input is not yet committed locally, so
/// retention must keep it.
async fn settle_protected_creation(outbox: &FleetOutbox, settled: DateTime<Utc>) -> RequestId {
    let (input, directory) = creation();
    outbox.enqueue_creation(&directory, &input).await.unwrap();
    outbox
        .mark_decided(&input.request_id, &accepted(), settled.timestamp_millis())
        .await
        .unwrap();
    input.request_id
}

struct RunningOutboxWorker {
    observation: WorkerObservation,
    clock: Arc<PausedWallClock>,
    prunes_entered_at: Arc<StdMutex<Vec<SyncTime>>>,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl RunningOutboxWorker {
    /// Starts the worker at `wall` and returns once its first pass has gone to sleep.
    async fn start(outbox: &FleetOutbox, wall: DateTime<Utc>) -> Self {
        let clock = Arc::new(PausedWallClock::starting_at(SyncTime::from_unix_millis(
            wall.timestamp_millis(),
        )));
        let (worker_loop, mut pass) = outbox_retention_loop(outbox, clock.clone());
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

    fn prunes(&self) -> usize {
        self.snapshot()
            .reads
            .iter()
            .filter(|read| **read == "fleet_outbox_retention.prune")
            .count()
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

/// Bounds, in real time, a wait for a pass the test expects; it only stops a hang.
const WAKE_GUARD: std::time::Duration = std::time::Duration::from_secs(30);

async fn open_outbox(directory: &Path) -> (FleetStore, FleetOutbox) {
    let store = FleetStore::open(&directory.join("fleet.sqlite"))
        .await
        .unwrap();
    let outbox = store.outbox();
    (store, outbox)
}

#[tokio::test]
async fn the_worker_arms_the_first_moment_the_rule_allows_and_deletes_only_then() {
    for (settled, first_deletable) in [
        // Mid-month: one millisecond after exactly two months.
        ("2026-01-15T10:00:00.000Z", "2026-03-15T10:00:00.001Z"),
        // Month end: the cutoff from late February clamps to 2025-12-28, so the row waits
        // for March.
        ("2025-12-31T12:00:00.000Z", "2026-03-01T00:00:00.000Z"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let (store, outbox) = open_outbox(directory.path()).await;
        let request = settle_rejected(&outbox, at(settled)).await;
        let worker = RunningOutboxWorker::start(&outbox, at("2026-02-01T00:00:00.000Z")).await;

        assert_eq!(worker.armed(), Some(at(first_deletable)), "{settled}");
        assert_eq!(worker.prunes(), 0, "nothing is deleted before it is due");
        assert!(outbox.request(&request).await.unwrap().is_some());

        worker.run_to_the_armed_time().await;

        worker.assert_last_prune_entered_at(at(first_deletable));
        assert!(
            outbox.request(&request).await.unwrap().is_none(),
            "{settled}"
        );
        assert_eq!(worker.prunes(), 1, "one DELETE, at the due time: {settled}");
        assert_eq!(worker.armed(), None, "nothing is left to arm: {settled}");
        worker.stop().await;
        store.close().await;
    }
}

#[tokio::test]
async fn a_protected_oldest_request_does_not_fire_and_completing_its_input_deletes_it() {
    let directory = tempfile::tempdir().unwrap();
    let (store, outbox) = open_outbox(directory.path()).await;
    let protected = settle_protected_creation(&outbox, at("2026-01-01T00:00:00.000Z")).await;
    let commits = outbox.subscribe_commits();
    let commits_before = *commits.borrow();
    let worker = RunningOutboxWorker::start(&outbox, at("2026-06-15T12:00:00.000Z")).await;

    assert_eq!(worker.armed(), None, "a kept request arms no timer");
    assert_eq!(worker.prunes(), 0);
    assert_eq!(
        *commits.borrow(),
        commits_before,
        "an idle pass writes nothing"
    );
    assert!(outbox.request(&protected).await.unwrap().is_some());

    let sleeps = worker.snapshot().sleeps;
    outbox.complete_creation_input(&protected).await.unwrap();
    tokio::time::timeout(WAKE_GUARD, worker.observation.asleep_at_least(sleeps + 1))
        .await
        .expect("completing the input wakes the worker");

    assert!(outbox.request(&protected).await.unwrap().is_none());
    assert_eq!(worker.prunes(), 1);
    worker.stop().await;
    store.close().await;
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
async fn set_delete_fault(directory: &Path, present: bool) {
    let mut connection = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new().filename(directory.join("fleet.sqlite")),
    )
    .await
    .unwrap();
    let statement = if present {
        "CREATE TRIGGER fail_outbox_prune BEFORE DELETE ON fleet_outbox_requests BEGIN SELECT RAISE(FAIL, 'outbox prune failure'); END"
    } else {
        "DROP TRIGGER fail_outbox_prune"
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
    let directory = tempfile::tempdir().unwrap();
    let (store, outbox) = open_outbox(directory.path()).await;
    let request = settle_rejected(&outbox, at("2026-01-01T00:00:00.000Z")).await;
    set_delete_fault(directory.path(), true).await;
    let wall = at("2026-06-15T12:00:00.000Z");

    let worker = RunningOutboxWorker::start(&outbox, wall).await;

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
    assert!(outbox.request(&request).await.unwrap().is_some());

    set_delete_fault(directory.path(), false).await;
    let retry_at = worker.armed().unwrap();
    worker.run_to_the_armed_time().await;

    worker.assert_last_prune_entered_at(retry_at);
    assert!(
        outbox.request(&request).await.unwrap().is_none(),
        "the retry needed only its time"
    );
    assert_eq!(worker.armed(), None);
    worker.stop().await;
    store.close().await;
}

#[tokio::test]
async fn protection_removed_before_the_due_time_arms_it_and_deletes_the_request_then() {
    let directory = tempfile::tempdir().unwrap();
    let (store, outbox) = open_outbox(directory.path()).await;
    let protected = settle_protected_creation(&outbox, at("2026-01-15T10:00:00.000Z")).await;
    let worker = RunningOutboxWorker::start(&outbox, at("2026-02-01T00:00:00.000Z")).await;
    assert_eq!(worker.armed(), None, "a kept request arms no timer");

    let sleeps = worker.snapshot().sleeps;
    outbox.complete_creation_input(&protected).await.unwrap();
    tokio::time::timeout(WAKE_GUARD, worker.observation.asleep_at_least(sleeps + 1))
        .await
        .expect("completing the input wakes the worker");

    assert_eq!(worker.armed(), Some(at("2026-03-15T10:00:00.001Z")));
    assert_eq!(worker.prunes(), 0, "not yet due");
    assert!(outbox.request(&protected).await.unwrap().is_some());

    worker.run_to_the_armed_time().await;

    worker.assert_last_prune_entered_at(at("2026-03-15T10:00:00.001Z"));
    assert!(outbox.request(&protected).await.unwrap().is_none());
    assert_eq!(worker.prunes(), 1);
    assert_eq!(worker.armed(), None);
    worker.stop().await;
    store.close().await;
}

#[tokio::test]
async fn a_commit_that_leaves_nothing_due_reads_the_due_time_and_deletes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let (store, outbox) = open_outbox(directory.path()).await;
    let older = settle_rejected(&outbox, at("2026-01-15T10:00:00.000Z")).await;
    // Enqueued before the worker parks, so the settlement below is the only commit after.
    let (_, pending) = creation();
    outbox.enqueue(&pending).await.unwrap();
    let newer = pending.request_id;
    let worker = RunningOutboxWorker::start(&outbox, at("2026-02-01T00:00:00.000Z")).await;
    let before = worker.snapshot();
    let due_reads = |snapshot: &LoopSnapshot| {
        snapshot
            .reads
            .iter()
            .filter(|read| **read == "fleet_outbox_retention.due")
            .count()
    };

    outbox
        .mark_decided(
            &newer,
            &RequestDecision::Rejected {
                reasons: Vec::new(),
            },
            at("2026-01-20T00:00:00.000Z").timestamp_millis(),
        )
        .await
        .unwrap();
    tokio::time::timeout(
        WAKE_GUARD,
        worker.observation.asleep_at_least(before.sleeps + 1),
    )
    .await
    .expect("the settlement wakes the worker");

    let after = worker.snapshot();
    assert!(
        due_reads(&after) > due_reads(&before),
        "the commit was read"
    );
    assert_eq!(
        worker.prunes(),
        0,
        "nothing was due, so nothing was deleted"
    );
    assert_eq!(after.armed_deadline, before.armed_deadline);
    assert!(outbox.request(&older).await.unwrap().is_some());
    assert!(outbox.request(&newer).await.unwrap().is_some());
    worker.stop().await;
    store.close().await;
}
