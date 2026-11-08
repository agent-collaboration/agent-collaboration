//! The retention worker on real stores: it arms one timer at the first moment a delete rule
//! allows a row to go, deletes nothing before it, and a commit that makes nothing due
//! deletes nothing.
//!
//! SQLite answers on background threads, so each test runs on real time until the worker
//! is asleep, and pauses Tokio only while the worker's own timer is the only one pending.
use super::AutomationRetentionWorker;
use crate::{LoopSnapshot, WorkerObservation, paused_wall_clock::PausedWallClock};
use agent_automation::{InstructionText, OperationId};
use automation_storage::{AutomationStore, InstructionUpdate};
use chrono::{DateTime, Utc};
use runtime_primitives::{Clock, SyncTime};
use sqlx::Connection;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex as StdMutex},
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::{Layer, layer::SubscriberExt};

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

struct RunningWorker {
    observation: WorkerObservation,
    clock: Arc<PausedWallClock>,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl RunningWorker {
    /// Starts the worker at `wall` and returns once its first pass has gone to sleep.
    async fn start(store: &Arc<Mutex<AutomationStore>>, wall: DateTime<Utc>) -> Self {
        Self::start_with(store, None, wall).await
    }

    async fn start_with(
        store: &Arc<Mutex<AutomationStore>>,
        interactions: Option<Arc<interaction_storage::InteractionStore>>,
        wall: DateTime<Utc>,
    ) -> Self {
        let clock = Arc::new(PausedWallClock::starting_at(SyncTime::from_unix_millis(
            wall.timestamp_millis(),
        )));
        let mut worker = AutomationRetentionWorker::new(Arc::clone(store), clock.clone());
        if let Some(interactions) = interactions {
            worker = worker.with_native_push_store(interactions);
        }
        let (worker_loop, mut pass) = worker.into_loop().await;
        let observation = worker_loop.observation();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn({
            let shutdown = shutdown.clone();
            async move { worker_loop.run(&mut pass, shutdown).await }
        });
        observation.asleep_at_least(1).await;
        Self {
            observation,
            clock,
            shutdown,
            task,
        }
    }

    fn snapshot(&self) -> LoopSnapshot {
        self.observation.snapshot()
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }
}

fn armed(snapshot: &LoopSnapshot) -> Option<DateTime<Utc>> {
    snapshot
        .armed_deadline
        .and_then(|deadline| DateTime::from_timestamp_millis(deadline.unix_millis()))
}

fn prunes(snapshot: &LoopSnapshot) -> Vec<&'static str> {
    snapshot
        .reads
        .iter()
        .copied()
        .filter(|read| read.ends_with(".prune"))
        .collect()
}

async fn open_store(directory: &Path) -> Arc<Mutex<AutomationStore>> {
    Arc::new(Mutex::new(
        AutomationStore::open(&directory.join("automation.sqlite"))
            .await
            .unwrap(),
    ))
}

/// Writes fixture rows beside the store before the worker starts, as an earlier process did.
async fn fixture_writer(directory: &Path) -> sqlx::SqliteConnection {
    sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(directory.join("automation.sqlite"))
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
    )
    .await
    .unwrap()
}

async fn insert_event(writer: &mut sqlx::SqliteConnection, id: &str, stamped: DateTime<Utc>) {
    sqlx::query("INSERT INTO automation_events(event_id,subject_kind,subject_id,event_kind,event_body_json,recorded_at_ms) VALUES (?,'fixture','owner','fixture','{}',?)")
        .bind(id)
        .bind(stamped.timestamp_millis())
        .execute(writer)
        .await
        .unwrap();
}

async fn revisions(writer: &mut sqlx::SqliteConnection) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM instruction_revisions")
        .fetch_one(writer)
        .await
        .unwrap()
}

async fn event_ids(writer: &mut sqlx::SqliteConnection) -> Vec<String> {
    sqlx::query_scalar("SELECT event_id FROM automation_events ORDER BY event_id")
        .fetch_all(writer)
        .await
        .unwrap()
}

/// Lets the paused clock reach the worker's timer and returns once that pass has slept.
async fn run_to_the_armed_time(worker: &RunningWorker) {
    let sleeps = worker.snapshot().sleeps;
    tokio::time::pause();
    worker.observation.asleep_at_least(sleeps + 1).await;
    tokio::time::resume();
}

async fn assert_deleted_at_its_cutoff(stamped: &str, wall: &str, expected_due: &str) {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let mut writer = fixture_writer(directory.path()).await;
    insert_event(&mut writer, "aging", at(stamped)).await;

    let worker = RunningWorker::start(&store, at(wall)).await;

    let parked = worker.snapshot();
    assert_eq!(
        armed(&parked),
        Some(at(expected_due)),
        "one timer at the strict cutoff"
    );
    assert!(
        prunes(&parked).is_empty(),
        "nothing was due before the cutoff"
    );
    assert_eq!(event_ids(&mut writer).await, vec!["aging"]);

    run_to_the_armed_time(&worker).await;

    let woke = worker.snapshot();
    let woke_at = DateTime::from_timestamp_millis(worker.clock.now().unix_millis()).unwrap();
    assert!(
        woke_at >= at(expected_due)
            && woke_at - at(expected_due) <= chrono::Duration::milliseconds(2),
        "the clock reached the cutoff and no further: {woke_at}"
    );
    assert_eq!(prunes(&woke), vec!["retention.events.prune"]);
    assert!(event_ids(&mut writer).await.is_empty());
    assert_eq!(
        armed(&woke),
        None,
        "with nothing left to age, no timer is held"
    );
    worker.stop().await;
}

#[tokio::test]
async fn a_row_goes_one_millisecond_after_exactly_two_calendar_months() {
    assert_deleted_at_its_cutoff(
        "2026-01-15T10:00:05.000Z",
        "2026-03-15T10:00:00.000Z",
        "2026-03-15T10:00:05.001Z",
    )
    .await;
}

#[tokio::test]
async fn a_month_end_row_waits_until_the_cutoff_leaves_the_short_month() {
    // The cutoff stands at 2025-12-28 for the whole of 2026-02-28.
    assert_deleted_at_its_cutoff(
        "2025-12-31T12:00:00.000Z",
        "2026-02-28T22:00:00.000Z",
        "2026-03-01T00:00:00.000Z",
    )
    .await;
}

#[tokio::test]
async fn a_protected_old_revision_holds_no_timer_and_goes_when_its_protection_is_removed() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let wall = at("2026-06-15T12:00:00.000Z");
    // The instruction's first revision is long past retention, but it is the current one.
    let instruction = store
        .lock()
        .await
        .create_instruction(
            &OperationId::generate(),
            &InstructionText::try_from("first revision".to_owned()).unwrap(),
            at("2026-01-01T00:00:00.000Z").timestamp_millis(),
        )
        .await
        .unwrap();
    let mut writer = fixture_writer(directory.path()).await;
    insert_event(&mut writer, "later", at("2026-04-15T13:00:00.000Z")).await;

    let worker = RunningWorker::start(&store, wall).await;

    let parked = worker.snapshot();
    assert_eq!(
        revisions(&mut writer).await,
        1,
        "the current revision stays"
    );
    assert_eq!(
        armed(&parked),
        Some(at("2026-06-15T13:00:00.001Z")),
        "the protected revision does not arm the timer; the later event does"
    );
    let passes = parked.passes;

    // Replacing the revision is the commit that removes its protection.
    store
        .lock()
        .await
        .update_instruction(&InstructionUpdate {
            operation_id: OperationId::generate(),
            instruction_id: instruction.instruction_id.clone(),
            expected_revision_id: instruction.revision_id.clone(),
            text: InstructionText::try_from("second revision".to_owned()).unwrap(),
            now_ms: wall.timestamp_millis(),
        })
        .await
        .unwrap();
    worker.observation.asleep_at_least(parked.sleeps + 1).await;

    let woke = worker.snapshot();
    assert!(woke.passes > passes, "the commit woke the worker");
    let since_park = LoopSnapshot {
        reads: woke.reads[parked.reads.len()..].to_vec(),
        ..LoopSnapshot::default()
    };
    assert_eq!(
        prunes(&since_park),
        vec!["retention.instruction_revisions.prune"],
        "the old revision went at its due time, which had already passed"
    );
    assert_eq!(
        revisions(&mut writer).await,
        1,
        "only the new revision is left"
    );
    assert_eq!(armed(&woke), Some(at("2026-06-15T13:00:00.001Z")));
    worker.stop().await;
}

#[tokio::test]
async fn a_commit_that_makes_nothing_due_reads_due_times_and_deletes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let wall = at("2026-06-15T12:00:00.000Z");
    let mut writer = fixture_writer(directory.path()).await;
    insert_event(&mut writer, "later", at("2026-04-15T13:00:00.000Z")).await;
    let worker = RunningWorker::start(&store, wall).await;
    let parked = worker.snapshot();

    // A new instruction commits fresh rows, none of them old enough to go.
    store
        .lock()
        .await
        .create_instruction(
            &OperationId::generate(),
            &InstructionText::try_from("fresh".to_owned()).unwrap(),
            wall.timestamp_millis(),
        )
        .await
        .unwrap();
    worker.observation.asleep_at_least(parked.sleeps + 1).await;

    let woke = worker.snapshot();
    let since_park = &woke.reads[parked.reads.len()..];
    assert!(
        !since_park.is_empty() && since_park.len() % 6 == 0,
        "whole passes of one due read per automation class: {since_park:?}"
    );
    assert!(
        since_park.iter().all(|read| read.ends_with(".due")),
        "a commit that leaves no class due runs no DELETE: {since_park:?}"
    );
    assert_eq!(armed(&woke), armed(&parked));
    assert_eq!(
        event_ids(&mut writer).await.len(),
        2,
        "the instruction's event and the fixture"
    );
    worker.stop().await;
}

/// Counts each value a named structured field takes across log events.
struct RetentionLog(Arc<StdMutex<HashMap<String, usize>>>);
impl<TSubscriber: tracing::Subscriber> Layer<TSubscriber> for RetentionLog {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, TSubscriber>,
    ) {
        #[derive(Default)]
        struct Fields {
            class: Option<String>,
            message: Option<String>,
        }
        impl tracing::field::Visit for Fields {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "retention_class" {
                    self.class = Some(value.to_owned());
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
        if let (Some(class), Some(message)) = (fields.class, fields.message) {
            *self
                .0
                .lock()
                .unwrap()
                .entry(format!("{class}: {message}"))
                .or_default() += 1;
        }
    }
}

#[tokio::test]
async fn a_failing_class_backs_off_on_its_own_while_the_other_classes_succeed() {
    let counts = Arc::new(StdMutex::new(HashMap::new()));
    let _trace = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(RetentionLog(counts.clone())),
    );
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let wall = at("2026-06-15T12:00:00.000Z");
    let mut writer = fixture_writer(directory.path()).await;
    insert_event(&mut writer, "aged", at("2026-01-01T00:00:00.000Z")).await;
    sqlx::query("INSERT INTO operation_receipts(operation_id,method_name,canonical_request,resource_id,operation_status,effect_evidence_json,final_result_json,committed_at_ms) VALUES ('aged-receipt','fixture',x'00','fixture','succeeded','[]','{}',?)")
        .bind(at("2026-01-01T00:00:00.000Z").timestamp_millis())
        .execute(&mut writer)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TRIGGER fail_receipt_prune BEFORE DELETE ON operation_receipts BEGIN SELECT RAISE(FAIL, 'receipt prune failure'); END")
        .execute(&mut writer)
        .await
        .unwrap();

    let worker = RunningWorker::start(&store, wall).await;

    assert!(
        event_ids(&mut writer).await.is_empty(),
        "the events class went on"
    );
    // The first retry is one second after the failed pass, which ran between the start and
    // the moment the worker parked; every later retry is timed from the one before.
    let first_retry = armed(&worker.snapshot()).unwrap();
    let parked_wall = DateTime::from_timestamp_millis(worker.clock.now().unix_millis()).unwrap();
    assert!(
        first_retry >= wall + chrono::Duration::seconds(1)
            && first_retry <= parked_wall + chrono::Duration::seconds(1),
        "the first retry is armed one second after the failure: {first_retry}"
    );
    let mut retries = Vec::new();
    let mut previous = first_retry;
    for _ in 0..4 {
        run_to_the_armed_time(&worker).await;
        let next = armed(&worker.snapshot()).unwrap();
        retries.push((next - previous).num_milliseconds());
        previous = next;
    }
    let failing = counts
        .lock()
        .unwrap()
        .get("operation receipts: retention prune failed; it retries with backoff")
        .copied();

    // Each retry doubles, timed from the pass that failed. That pass starts at the armed
    // time or up to 2 ms after it: the paused timer rounds up to its 1 ms tick, from a wall
    // clock that kept a fraction of a real millisecond from before the pause.
    for (retry, expected) in retries.iter().zip([2_000, 4_000, 8_000, 16_000]) {
        assert!(
            (expected..=expected + 2).contains(retry),
            "retries {retries:?} should double from 2 s"
        );
    }
    assert_eq!(failing, Some(1), "one warning for the run of failures");
    sqlx::query("DROP TRIGGER fail_receipt_prune")
        .execute(&mut writer)
        .await
        .unwrap();
    run_to_the_armed_time(&worker).await;
    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM operation_receipts")
        .fetch_one(&mut writer)
        .await
        .unwrap();
    assert_eq!(receipts, 0, "the retry needed no new write, only its time");
    assert_eq!(
        counts
            .lock()
            .unwrap()
            .get("operation receipts: retention prune recovered")
            .copied(),
        Some(1)
    );
    worker.stop().await;
}

#[tokio::test]
async fn a_terminal_push_goes_at_its_strict_cutoff_and_a_pending_one_holds_no_timer() {
    use collaboration_protocol::{
        EndpointId, EndpointRef, PushHeaderFacts, PushId, PushKind, PushOrigin, PushRecordDraft,
        SessionId, SessionRef, UuidIdentity,
    };
    let session = |endpoint: &str, id: &str| SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from("018f47d2-24d5-7a68-b9ec-6f759c39458f".to_owned())
                .unwrap(),
            endpoint_id: EndpointId::try_from(endpoint.to_owned()).unwrap(),
        },
        session_id: SessionId::try_from(id.to_owned()).unwrap(),
    };
    let draft = |created_at: DateTime<Utc>| PushRecordDraft {
        mode: Some(collaboration_protocol::MessageDelivery::Auto),
        guard: None,
        push_id: PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap(),
        kind: PushKind::DirectMessage,
        origin: PushOrigin::Session(session("claude-local", "sender")),
        origin_router_ref: None,
        target: session("codex-local", "target"),
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        },
        body: Some("aging push".to_owned()),
        activity: None,
        created_at,
    };
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path()).await;
    let interaction_path = directory.path().join("interaction.sqlite");
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(&interaction_path)
            .await
            .unwrap(),
    );
    // The older push is still pending, so it is protected; the newer one is delivered.
    let pending = draft(at("2026-01-10T00:00:00.000Z"));
    let delivered = draft(at("2026-01-15T10:00:05.000Z"));
    let delivered_id = delivered.push_id.clone();
    interactions.insert_push_record(pending).await.unwrap();
    interactions.insert_push_record(delivered).await.unwrap();
    let mut interaction_writer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new().filename(&interaction_path),
    )
    .await
    .unwrap();
    sqlx::query::<sqlx_turso::Turso>(
        "UPDATE router_pushes SET delivery_state='delivered' WHERE push_id=?",
    )
    .bind(delivered_id.as_str())
    .execute(&mut interaction_writer)
    .await
    .unwrap();

    let worker = RunningWorker::start_with(
        &store,
        Some(interactions.clone()),
        at("2026-03-15T10:00:00.000Z"),
    )
    .await;

    let parked = worker.snapshot();
    assert_eq!(
        armed(&parked),
        Some(at("2026-03-15T10:00:05.001Z")),
        "the delivered push arms the timer; the older pending push does not"
    );
    assert!(prunes(&parked).is_empty());
    run_to_the_armed_time(&worker).await;
    let woke = worker.snapshot();
    assert_eq!(prunes(&woke), vec!["retention.interaction_records.prune"]);
    assert!(
        interactions
            .get_push_record(&delivered_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(armed(&woke), None, "the pending push holds no timer");
    worker.stop().await;
    interaction_writer.close().await.unwrap();
}
