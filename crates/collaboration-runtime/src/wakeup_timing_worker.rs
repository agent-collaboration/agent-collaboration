//! Host-owned timer task turns due wake-ups into durable obligations, independently of native I/O.
//!
//! The worker sleeps until an automation commit, a configuration change, a delivery that
//! made progress, failed or freed a slot, or its next due time: an active wake's next
//! firing, an active or paused wake's expiry, or a delivery's eligibility.
use crate::{
    BatchOutcome, WakeDrivenWorkerLoop, WorkerObservation, WorkerPass,
    keyed_worker_tasks::KeyedWorkerTasks,
};
use agent_automation::DeliveryId;
use automation_storage::{AutomationStore, StorageError};
use collaboration_protocol::SavedMessage;
use runtime_primitives::SyncTime;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// At most this many deliveries dispatch at once.
const MAX_ACTIVE_DISPATCHES: usize = 16;
/// The store returns inventories in batches of this size.
const INVENTORY_BATCH: u32 = 100;

pub struct WakeTimingWorker {
    store: Arc<Mutex<AutomationStore>>,
    native: crate::wakeup_delivery_sender::WakeDeliverySender,
    observation: WorkerObservation,
    #[cfg(any(test, feature = "test-support"))]
    accepted_observer: Option<tokio::sync::mpsc::UnboundedSender<DeliveryId>>,
    #[cfg(test)]
    dispatch_gate: Option<crate::keyed_worker_tasks::TaskGate>,
}
impl WakeTimingWorker {
    pub fn new(
        store: Arc<Mutex<AutomationStore>>,
        delivery: Arc<dyn collaboration_contracts::SessionMessageDelivery>,
        configuration: crate::AutomationConfigurationHandle,
        machine_identity: collaboration_contracts::MachineIdentity,
        interactions: Arc<interaction_storage::InteractionStore>,
        clock: Arc<dyn runtime_primitives::Clock>,
    ) -> Self {
        let native = crate::wakeup_delivery_sender::WakeDeliverySender {
            delivery,
            configuration,
            machine_identity,
            interactions,
            clock,
        };
        Self {
            store,
            native,
            observation: WorkerObservation::default(),
            #[cfg(any(test, feature = "test-support"))]
            accepted_observer: None,
            #[cfg(test)]
            dispatch_gate: None,
        }
    }

    /// Holds every dispatch task after its work and before it reports, until the test
    /// releases the gate; `arrivals` counts the dispatches held.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn hold_dispatch_reports(
        mut self,
        gate: Arc<Mutex<()>>,
        arrivals: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        self.dispatch_gate = Some(crate::keyed_worker_tasks::TaskGate::counting(
            crate::keyed_worker_tasks::TaskGatePoint::BeforeReport,
            gate,
            arrivals,
        ));
        self
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn observe_accepted_deliveries(
        &mut self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<DeliveryId> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        self.accepted_observer = Some(sender);
        receiver
    }

    /// What the worker's loop reports; under test-support a test reads it.
    #[must_use]
    pub fn observation(&self) -> WorkerObservation {
        self.observation.clone()
    }

    /// Start only after the Host owns its listeners and previous workers have retired.
    pub async fn run(self, shutdown: CancellationToken) {
        let commits = self.store.lock().await.subscribe_commits();
        let dispatch_commits = self.store.lock().await.subscribe_commits();
        let configuration = self.native.configuration.subscribe_changes();
        // The pass tells an event from its own dispatches' reports through its own copies.
        let events = vec![commits.clone(), configuration.clone()];
        #[cfg_attr(
            not(test),
            expect(unused_mut, reason = "only tests hold dispatch tasks")
        )]
        let (mut deliveries, delivery_wakes) = KeyedWorkerTasks::new(
            MAX_ACTIVE_DISPATCHES,
            dispatch_commits,
            "automation delivery task stopped; persisted attempt must be inspected before replay",
        );
        #[cfg(test)]
        deliveries.hold(self.dispatch_gate);
        #[cfg(any(test, feature = "test-support"))]
        self.observation
            .track_running_tasks(deliveries.running_tasks());
        let worker = WakeDrivenWorkerLoop::new("wake-ups", Arc::clone(&self.native.clock))
            .with_observation(self.observation)
            .wake_on(commits)
            .wake_on(configuration)
            .wake_on(delivery_wakes);
        let mut pass = WakePass {
            store: self.store,
            native: self.native,
            deliveries,
            events,
            armed: None,
            recovered: false,
            #[cfg(any(test, feature = "test-support"))]
            accepted_observer: self.accepted_observer,
        };
        worker.run(&mut pass, shutdown).await;
        pass.deliveries.shut_down().await;
    }
}

struct WakePass {
    store: Arc<Mutex<AutomationStore>>,
    native: crate::wakeup_delivery_sender::WakeDeliverySender,
    deliveries: KeyedWorkerTasks<DeliveryId>,
    /// Automation commits and configuration changes: anything that may concern a
    /// delivery, unlike a dispatch's report or a freed slot.
    events: Vec<tokio::sync::watch::Receiver<u64>>,
    /// The due time this worker last armed; a wake at or after it is an event.
    armed: Option<SyncTime>,
    /// Interrupted deliveries from a previous process are recovered by the first pass.
    recovered: bool,
    #[cfg(any(test, feature = "test-support"))]
    accepted_observer: Option<tokio::sync::mpsc::UnboundedSender<DeliveryId>>,
}

impl WakePass {
    async fn evaluate_due(
        &self,
        now_ms: i64,
        observation: &WorkerObservation,
    ) -> Result<usize, StorageError> {
        observation.record_read("due wake-ups");
        let ids = self
            .store
            .lock()
            .await
            .due_wakeup_ids(now_ms, INVENTORY_BATCH)
            .await?;
        for id in &ids {
            self.store
                .lock()
                .await
                .evaluate_wakeup::<SavedMessage>(
                    id,
                    now_ms,
                    crate::wakeup_delivery_sender::build_wake_push_draft,
                )
                .await?;
        }
        Ok(ids.len())
    }

    async fn dispatch_eligible(
        &mut self,
        now: SyncTime,
        observation: &WorkerObservation,
    ) -> Result<(), StorageError> {
        observation.record_read("eligible deliveries");
        let ids = self
            .store
            .lock()
            .await
            .eligible_delivery_ids(now.unix_millis(), INVENTORY_BATCH)
            .await?;
        for id in ids {
            if self.deliveries.at_capacity(now).await {
                // A dispatch that finishes now frees a slot and wakes the loop.
                return Ok(());
            }
            if self.deliveries.is_busy(&id, now) {
                continue;
            }
            let store = Arc::clone(&self.store);
            let native = self.native.clone();
            #[cfg(any(test, feature = "test-support"))]
            let observer = self.accepted_observer.clone();
            self.deliveries.spawn(id.clone(), async move {
                #[cfg(any(test, feature = "test-support"))]
                let observed = (Arc::clone(&store), id.clone());
                let result = native.dispatch(store, id).await;
                #[cfg(any(test, feature = "test-support"))]
                if result.is_ok()
                    && let Some(observer) = observer
                {
                    // A dispatch can return without claiming an unadmitted push.
                    // Read once after it returns; only a committed acceptance is a fence.
                    let accepted = observed
                        .0
                        .lock()
                        .await
                        .read_delivery::<
                            collaboration_protocol::SessionRef,
                            collaboration_protocol::CodexGeneration,
                            serde_json::Value,
                        >(&observed.1)
                        .await
                        .is_ok_and(|record| {
                            record.status == agent_automation::DeliveryStatus::Accepted
                        });
                    if accepted {
                        let _ = observer.send(observed.1);
                    }
                }
                result.is_ok()
            });
        }
        Ok(())
    }
}

impl WorkerPass for WakePass {
    type Error = StorageError;

    async fn drain_batch(
        &mut self,
        now: SyncTime,
        observation: &WorkerObservation,
    ) -> Result<BatchOutcome, StorageError> {
        // Events are taken before reports, as the scheduler does; with no cursor, a
        // dispatch an event overtook only needs the wake its report sends.
        let mut event = self.armed.take_if(|due| *due <= now).is_some();
        for source in &mut self.events {
            event |= source.has_changed().unwrap_or(false);
            source.borrow_and_update();
        }
        if event {
            self.deliveries.event_arrived();
        }
        self.deliveries.reap(now);
        self.deliveries.take_sweep_request();
        let now_ms = now.unix_millis();
        if !self.recovered {
            observation.record_read("interrupted deliveries");
            self.store
                .lock()
                .await
                .recover_interrupted_deliveries(now_ms)
                .await?;
            self.recovered = true;
        }
        if self.native.configuration.current().await.is_none() {
            // A configuration change wakes the loop again.
            return Ok(BatchOutcome::Drained);
        }
        let evaluated = self.evaluate_due(now_ms, observation).await?;
        self.dispatch_eligible(now, observation).await?;
        Ok(if evaluated == INVENTORY_BATCH as usize {
            BatchOutcome::MoreWork
        } else {
            BatchOutcome::Drained
        })
    }

    async fn next_due(
        &mut self,
        now: SyncTime,
        observation: &WorkerObservation,
    ) -> Result<Option<SyncTime>, StorageError> {
        if self.native.configuration.current().await.is_none() {
            return Ok(None);
        }
        observation.record_read("wake-up next due");
        let stored = self
            .store
            .lock()
            .await
            .wake_worker_next_due_at_ms(now.unix_millis())
            .await?
            .map(SyncTime::from_unix_millis);
        self.armed = match (stored, self.deliveries.next_retry(now)) {
            (Some(stored), Some(retry)) => Some(stored.min(retry)),
            (stored, retry) => stored.or(retry),
        };
        Ok(self.armed)
    }
}

#[cfg(test)]
#[path = "wakeup_timing_worker_test.rs"]
mod tests;
