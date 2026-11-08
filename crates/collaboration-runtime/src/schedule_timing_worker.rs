//! Host-composed schedule timing and bounded Run steps; no overlap policy is delegated to native queues.
//!
//! The scheduler sleeps until an automation commit, a configuration change, a completion
//! a running Run waits for, a step that failed or freed a slot, or its next due time: a
//! schedule's next occurrence, a Run's execution deadline or a summary's deadline. A step
//! that finds its turn still running writes nothing and so wakes nothing.
use crate::keyed_worker_tasks::KeyedWorkerTasks;
use crate::{
    BatchOutcome, WakeDrivenWorkerLoop, WorkerObservation, WorkerPass,
    scheduled_run_worker::ScheduledRunWorker,
};
use agent_automation::{RunId, ScheduleId};
use automation_storage::{AutomationStore, StorageError};
use collaboration_protocol::{EndpointRef, SessionRef};
use runtime_primitives::SyncTime;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;

/// At most this many Run steps run at once.
const MAX_ACTIVE_STEPS: usize = 16;
/// The store returns inventories in batches of this size.
const INVENTORY_BATCH: usize = 100;
const STEP_TIMEOUT: Duration = Duration::from_secs(35);

pub struct ScheduleTimingWorker {
    store: Arc<Mutex<AutomationStore>>,
    runner: ScheduledRunWorker,
    completion_sources: Vec<watch::Receiver<u64>>,
    observation: WorkerObservation,
    #[cfg(test)]
    step_gate: Option<crate::keyed_worker_tasks::TaskGate>,
}

impl ScheduleTimingWorker {
    pub fn new(
        store: Arc<Mutex<AutomationStore>>,
        interactions: Arc<interaction_storage::InteractionStore>,
        execution: Arc<dyn collaboration_contracts::ScheduledRunExecution>,
        backend: Option<crate::NativeControlBackend>,
        configuration: crate::AutomationConfigurationHandle,
        machine_identity: collaboration_contracts::MachineIdentity,
        clock: Arc<dyn runtime_primitives::Clock>,
    ) -> Self {
        Self {
            store: Arc::clone(&store),
            runner: ScheduledRunWorker {
                store,
                interactions,
                execution,
                backend,
                configuration,
                machine_identity,
                clock,
            },
            completion_sources: Vec::new(),
            observation: WorkerObservation::default(),
            #[cfg(test)]
            step_gate: None,
        }
    }

    /// Wakes the scheduler on a completion its running Runs wait for: the lifecycle
    /// journal's commits (a native `turn/completed`), the provider supervisor's
    /// completions, or the observation retry while lifecycle observation is unavailable.
    /// Subscribe the receiver before the scheduler starts.
    #[must_use]
    pub fn wake_on_completions(mut self, source: watch::Receiver<u64>) -> Self {
        self.completion_sources.push(source);
        self
    }

    /// Holds every step task after it has reported and woken the loop, until the test
    /// releases the gate: the task's completion then lags its report.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn hold_step_exits(mut self, gate: Arc<Mutex<()>>) -> Self {
        self.step_gate = Some(crate::keyed_worker_tasks::TaskGate::new(
            crate::keyed_worker_tasks::TaskGatePoint::AfterReport,
            gate,
        ));
        self
    }

    /// Holds every step task after its work, which may have committed, and before it
    /// reports, until the test releases the gate.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn hold_step_reports(mut self, gate: Arc<Mutex<()>>) -> Self {
        self.step_gate = Some(crate::keyed_worker_tasks::TaskGate::new(
            crate::keyed_worker_tasks::TaskGatePoint::BeforeReport,
            gate,
        ));
        self
    }

    /// What the scheduler's loop reports; under test-support a test reads it.
    #[must_use]
    pub fn observation(&self) -> WorkerObservation {
        self.observation.clone()
    }

    pub async fn run(self, shutdown: CancellationToken) {
        let commits = self.store.lock().await.subscribe_commits();
        let step_commits = self.store.lock().await.subscribe_commits();
        let configuration = self.runner.configuration.subscribe_changes();
        #[cfg_attr(not(test), expect(unused_mut, reason = "only tests hold step tasks"))]
        let (mut steps, step_wakes) = KeyedWorkerTasks::new(
            MAX_ACTIVE_STEPS,
            step_commits,
            "scheduled Run step unavailable; retained state remains inspectable and the step retries with backoff",
        );
        #[cfg(test)]
        steps.hold(self.step_gate);
        let mut worker = WakeDrivenWorkerLoop::new("schedules", Arc::clone(&self.runner.clock))
            .with_observation(self.observation)
            .wake_on(commits)
            .wake_on(configuration)
            .wake_on(step_wakes);
        for source in self.completion_sources {
            worker = worker.wake_on(source);
        }
        let mut pass = SchedulePass {
            store: self.store,
            runner: self.runner,
            steps,
            run_cursor: None,
            waiting_cursor: None,
        };
        worker.run(&mut pass, shutdown).await;
        pass.steps.shut_down().await;
    }
}

struct SchedulePass {
    store: Arc<Mutex<AutomationStore>>,
    runner: ScheduledRunWorker,
    /// Run steps in flight, their reports and their per-Run retries.
    steps: KeyedWorkerTasks<RunId>,
    // Scan progress is only a fairness hint; durable Run state still owns exclusion.
    run_cursor: Option<RunId>,
    waiting_cursor: Option<ScheduleId>,
}

impl SchedulePass {
    async fn enqueue_due(
        &mut self,
        now_ms: i64,
        observation: &WorkerObservation,
    ) -> Result<usize, StorageError> {
        observation.record_read("due schedules");
        let due = self.store.lock().await.due_schedule_ids(now_ms).await?;
        for id in &due {
            self.store
                .lock()
                .await
                .enqueue_due_run::<SessionRef, EndpointRef>(id, now_ms)
                .await?;
        }
        Ok(due.len())
    }

    async fn admit_waiting(
        &mut self,
        now_ms: i64,
        observation: &WorkerObservation,
    ) -> Result<usize, StorageError> {
        observation.record_read("waiting schedules");
        let waiting = self
            .store
            .lock()
            .await
            .waiting_schedule_ids(self.waiting_cursor.as_ref())
            .await?;
        let read = waiting.len();
        for id in waiting {
            self.waiting_cursor = Some(id.clone());
            let configuration_lease = self.runner.configuration.admission_lease().await;
            if configuration_lease.configuration().is_none() {
                // A configuration change wakes the loop again.
                break;
            }
            self.store
                .lock()
                .await
                .admit_waiting_run::<SessionRef, EndpointRef>(&id, now_ms)
                .await?;
        }
        if read < INVENTORY_BATCH {
            self.waiting_cursor = None;
        }
        Ok(read)
    }

    async fn start_observable_steps(
        &mut self,
        now: SyncTime,
        observation: &WorkerObservation,
    ) -> Result<RunScan, StorageError> {
        observation.record_read("observable runs");
        let ids = self
            .store
            .lock()
            .await
            .observable_run_ids(self.run_cursor.as_ref())
            .await?;
        let read = ids.len();
        for id in ids {
            if self.steps.at_capacity() {
                // A step that finishes now frees a slot and wakes the loop.
                return Ok(RunScan::AtCapacity);
            }
            self.run_cursor = Some(id.clone());
            if !self.steps.is_busy(&id, now) {
                let runner = self.runner.clone();
                self.steps.spawn(id.clone(), async move {
                    matches!(
                        tokio::time::timeout(
                            STEP_TIMEOUT,
                            Box::pin(runner.step_reporting_client(id))
                        )
                        .await,
                        Ok(Ok(crate::scheduled_run_worker::StepProgress::Stepped))
                    )
                });
            }
        }
        if read < INVENTORY_BATCH {
            self.run_cursor = None;
            return Ok(RunScan::Complete);
        }
        Ok(RunScan::MoreRuns)
    }
}

enum RunScan {
    Complete,
    MoreRuns,
    AtCapacity,
}

impl WorkerPass for SchedulePass {
    type Error = StorageError;

    async fn drain_batch(
        &mut self,
        now: SyncTime,
        observation: &WorkerObservation,
    ) -> Result<BatchOutcome, StorageError> {
        self.steps.reap(now);
        let now_ms = now.unix_millis();
        let due = self.enqueue_due(now_ms, observation).await?;
        let waiting = self.admit_waiting(now_ms, observation).await?;
        let runs = self.start_observable_steps(now, observation).await?;
        let more = due == INVENTORY_BATCH
            || waiting == INVENTORY_BATCH
            || matches!(runs, RunScan::MoreRuns);
        Ok(if more {
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
        observation.record_read("scheduler next due");
        let stored = self
            .store
            .lock()
            .await
            .scheduler_next_due_at_ms(now.unix_millis())
            .await?
            .map(SyncTime::from_unix_millis);
        Ok(match (stored, self.steps.next_retry(now)) {
            (Some(stored), Some(retry)) => Some(stored.min(retry)),
            (stored, retry) => stored.or(retry),
        })
    }
}

#[cfg(test)]
#[path = "schedule_timing_worker_test.rs"]
mod tests;
