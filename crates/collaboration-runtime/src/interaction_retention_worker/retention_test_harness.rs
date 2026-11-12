//! Retention on real stores, driven only by its injected manual clock.
use super::InteractionRetentionWorker;
use crate::{LoopSnapshot, WorkerObservation};
use chrono::{DateTime, Utc};
use router_clock::{ManualClock, RuntimeClock};
use runtime_primitives::SyncTime;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex as StdMutex},
};
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
    pub(super) async fn start(
        interactions: Option<Arc<interaction_storage::InteractionStore>>,
        broker: Option<Arc<crate::ServiceInteractionBroker>>,
        wall: DateTime<Utc>,
    ) -> Self {
        let clock = Arc::new(
            ManualClock::starting_at(SyncTime::from_unix_millis(wall.timestamp_millis())).unwrap(),
        );
        let mut worker = InteractionRetentionWorker::new(clock.clone());
        if let Some(interactions) = interactions {
            worker = worker.with_interaction_store(interactions);
        }
        if let Some(broker) = broker {
            worker = worker.with_interaction_broker(broker);
        }
        let (worker_loop, mut pass) = worker.into_loop();
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

pub(super) async fn open_interactions(
    directory: &Path,
) -> Arc<interaction_storage::InteractionStore> {
    Arc::new(
        interaction_storage::InteractionStore::open(directory.join("interaction.sqlite"))
            .await
            .unwrap(),
    )
}

/// Stores an approval route whose last use is `last_used`.
pub(super) async fn insert_route(
    store: &interaction_storage::InteractionStore,
    thread_id: &str,
    last_used: DateTime<Utc>,
) {
    store
        .upsert_approval_route(
            &interaction_storage::StoredApprovalRoute {
                thread_id: thread_id.to_owned(),
                route_json: "{}".to_owned(),
            },
            last_used.timestamp_millis(),
        )
        .await
        .unwrap();
}

/// The stored approval routes, read beside the store without touching their last use.
pub(super) async fn route_ids(directory: &Path) -> Vec<String> {
    use sqlx::Connection;
    let mut reader = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(directory.join("interaction.sqlite"))
            .read_only(true),
    )
    .await
    .unwrap();
    let ids = sqlx::query_scalar::<sqlx_turso::Turso, String>(
        "SELECT thread_id FROM approval_routes ORDER BY thread_id",
    )
    .fetch_all(&mut reader)
    .await
    .unwrap();
    reader.close().await.unwrap();
    ids
}
