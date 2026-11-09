//! Retention on real stores, driven only by its injected manual clock.
use super::AutomationRetentionWorker;
use crate::{LoopSnapshot, WorkerObservation};
use automation_storage::AutomationStore;
use chrono::{DateTime, Utc};
use router_clock::{ManualClock, RuntimeClock};
use runtime_primitives::SyncTime;
use sqlx::Connection;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex as StdMutex},
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::Layer;

pub(super) fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

pub(super) struct RunningWorker {
    pub(super) observation: WorkerObservation,
    pub(super) clock: Arc<ManualClock>,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl RunningWorker {
    /// Starts the worker at `wall` and returns once its first pass has gone to sleep.
    pub(super) async fn start(store: &Arc<Mutex<AutomationStore>>, wall: DateTime<Utc>) -> Self {
        Self::start_with(store, None, wall).await
    }

    pub(super) async fn start_with(
        store: &Arc<Mutex<AutomationStore>>,
        interactions: Option<Arc<interaction_storage::InteractionStore>>,
        wall: DateTime<Utc>,
    ) -> Self {
        Self::start_full(store, interactions, None, wall).await
    }

    pub(super) async fn start_full(
        store: &Arc<Mutex<AutomationStore>>,
        interactions: Option<Arc<interaction_storage::InteractionStore>>,
        broker: Option<Arc<crate::ServiceInteractionBroker>>,
        wall: DateTime<Utc>,
    ) -> Self {
        let clock = Arc::new(
            ManualClock::starting_at(SyncTime::from_unix_millis(wall.timestamp_millis())).unwrap(),
        );
        let mut worker = AutomationRetentionWorker::new(Arc::clone(store), clock.clone());
        if let Some(interactions) = interactions {
            worker = worker.with_native_push_store(interactions);
        }
        if let Some(broker) = broker {
            worker = worker.with_interaction_broker(broker);
        }
        let (worker_loop, mut pass) = worker.into_loop().await;
        let observation = worker_loop.observation();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn({
            let shutdown = shutdown.clone();
            async move { worker_loop.run(&mut pass, shutdown).await }
        });
        observation.asleep_at_least(1).await;
        if let Some(deadline) = observation.snapshot().armed_deadline {
            assert!(
                clock.has_not_due_check_now(clock.wall_deadline(deadline)),
                "worker must register its injected sleep"
            );
        }
        Self {
            observation,
            clock,
            shutdown,
            task,
        }
    }

    pub(super) fn snapshot(&self) -> LoopSnapshot {
        self.observation.snapshot()
    }

    pub(super) async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }
}

pub(super) fn armed(snapshot: &LoopSnapshot) -> Option<DateTime<Utc>> {
    snapshot
        .armed_deadline
        .and_then(|deadline| DateTime::from_timestamp_millis(deadline.unix_millis()))
}

pub(super) fn prunes(snapshot: &LoopSnapshot) -> Vec<&'static str> {
    snapshot
        .reads
        .iter()
        .copied()
        .filter(|read| read.ends_with(".prune"))
        .collect()
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

pub(super) async fn event_ids(writer: &mut sqlx::SqliteConnection) -> Vec<String> {
    sqlx::query_scalar("SELECT event_id FROM automation_events ORDER BY event_id")
        .fetch_all(writer)
        .await
        .unwrap()
}

/// Advances the manual clock to the armed deadline and awaits the resulting pass.
pub(super) async fn run_to_the_armed_time(worker: &RunningWorker) {
    let sleeps = worker.snapshot().sleeps;
    worker
        .clock
        .advance_to(worker.snapshot().armed_deadline.unwrap())
        .unwrap();
    worker.observation.asleep_at_least(sleeps + 1).await;
}

/// Counts each value a named structured field takes across log events.
pub(super) struct RetentionLog(pub(super) Arc<StdMutex<HashMap<String, usize>>>);
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

pub(super) fn aging_push(created_at: DateTime<Utc>) -> collaboration_protocol::PushRecordDraft {
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
    PushRecordDraft {
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
    }
}
