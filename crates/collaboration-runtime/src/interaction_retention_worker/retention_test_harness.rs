//! Retention on a real interaction store and broker, driven only by its injected manual clock.
use super::InteractionRetentionWorker;
use crate::{LoopSnapshot, WakeDrivenWorkerLoop, WorkerObservation};
use chrono::{DateTime, Utc};
use router_clock::{ManualClock, RuntimeClock};
use runtime_primitives::SyncTime;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::Layer;

pub(crate) fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

/// A worker loop running on its own task, with the manual clock it sleeps on.
pub(crate) struct RunningWorker {
    pub(crate) observation: WorkerObservation,
    pub(crate) clock: Arc<ManualClock>,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl RunningWorker {
    /// Starts the interaction retention worker at `wall` and returns once its first pass has
    /// gone to sleep.
    pub(crate) async fn start(
        broker: &Arc<crate::ServiceInteractionBroker>,
        wall: DateTime<Utc>,
    ) -> Self {
        let clock = manual_clock_at(wall);
        let (worker_loop, pass) =
            InteractionRetentionWorker::new(Arc::clone(broker), clock.clone()).into_loop();
        Self::spawn(worker_loop, pass, clock).await
    }

    /// Runs `worker_loop` with `pass` and waits for the first pass to go to sleep.
    pub(crate) async fn spawn<TPass: crate::WorkerPass + Send + 'static>(
        worker_loop: WakeDrivenWorkerLoop,
        mut pass: TPass,
        clock: Arc<ManualClock>,
    ) -> Self {
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

    pub(crate) fn snapshot(&self) -> LoopSnapshot {
        self.observation.snapshot()
    }

    pub(crate) async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }
}

pub(crate) fn manual_clock_at(wall: DateTime<Utc>) -> Arc<ManualClock> {
    Arc::new(ManualClock::starting_at(SyncTime::from_unix_millis(wall.timestamp_millis())).unwrap())
}

pub(crate) fn armed(snapshot: &LoopSnapshot) -> Option<DateTime<Utc>> {
    snapshot
        .armed_deadline
        .and_then(|deadline| DateTime::from_timestamp_millis(deadline.unix_millis()))
}

pub(crate) fn prunes(snapshot: &LoopSnapshot) -> Vec<&'static str> {
    snapshot
        .reads
        .iter()
        .copied()
        .filter(|read| read.ends_with(".prune"))
        .collect()
}

/// Advances the manual clock to the armed deadline and awaits the resulting pass.
pub(crate) async fn run_to_the_armed_time(worker: &RunningWorker) {
    let sleeps = worker.snapshot().sleeps;
    worker
        .clock
        .advance_to(worker.snapshot().armed_deadline.unwrap())
        .unwrap();
    worker.observation.asleep_at_least(sleeps + 1).await;
}

/// Counts each value a named structured field takes across log events.
pub(crate) struct RetentionLog(pub(crate) Arc<StdMutex<HashMap<String, usize>>>);
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

pub(crate) fn aging_push(created_at: DateTime<Utc>) -> collaboration_protocol::PushRecordDraft {
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

/// Inserts `draft` and settles it through the store, as an earlier delivery left it.
pub(crate) async fn insert_settled_push(
    store: &interaction_storage::InteractionStore,
    draft: collaboration_protocol::PushRecordDraft,
) -> collaboration_protocol::PushId {
    let push_id = draft.push_id.clone();
    let settled_at = draft.created_at;
    store.insert_push_record(draft).await.unwrap();
    store.mark_push_attempted(&push_id).await.unwrap();
    store
        .settle_push_record(
            &push_id,
            collaboration_protocol::DeliveryReceipt {
                outcome: collaboration_protocol::DeliveryOutcome::PeerMessageWritten,
                reachability: None,
                client: None,
            },
            settled_at,
        )
        .await
        .unwrap();
    push_id
}
