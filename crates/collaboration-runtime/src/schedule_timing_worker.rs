//! Host-composed schedule timing and bounded Run steps; no overlap policy is delegated to native queues.
//!
//! The scheduler sleeps until an automation commit, a configuration change, a completion
//! a running Run waits for, a step that failed or freed a slot, or its next due time: a
//! schedule's next occurrence, a Run's execution deadline or a summary's deadline. A step
//! that finds its turn still running writes nothing and so wakes nothing.
use crate::{
    BatchOutcome, WakeDrivenWorkerLoop, WorkerObservation, WorkerPass,
    scheduled_run_worker::ScheduledRunWorker,
};
use agent_automation::{RunId, ScheduleId};
use automation_storage::{AutomationStore, StorageError};
use collaboration_protocol::{EndpointRef, SessionRef};
use runtime_primitives::SyncTime;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;

/// At most this many Run steps run at once.
const MAX_ACTIVE_STEPS: usize = 16;
/// The store returns inventories in batches of this size.
const INVENTORY_BATCH: usize = 100;
const STEP_TIMEOUT: Duration = Duration::from_secs(35);
/// A failed Run step retries 1 s, doubling to 30 s, until a step succeeds.
const FAILED_STEP_BACKOFF: runtime_primitives::Backoff = runtime_primitives::Backoff {
    initial: Duration::from_secs(1),
    max: Duration::from_secs(30),
    factor: 2,
};

pub struct ScheduleTimingWorker {
    store: Arc<Mutex<AutomationStore>>,
    runner: ScheduledRunWorker,
    completion_sources: Vec<watch::Receiver<u64>>,
    observation: WorkerObservation,
    #[cfg(test)]
    step_exit_gate: Option<Arc<Mutex<()>>>,
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
            step_exit_gate: None,
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
        self.step_exit_gate = Some(gate);
        self
    }

    /// What the scheduler's loop reports; under test-support a test reads it.
    #[must_use]
    pub fn observation(&self) -> WorkerObservation {
        self.observation.clone()
    }

    pub async fn run(self, shutdown: CancellationToken) {
        let commits = self.store.lock().await.subscribe_commits();
        let configuration = self.runner.configuration.subscribe_changes();
        let (step_outcomes, step_wakes) = watch::channel(0);
        let (step_reports, finished_steps) = tokio::sync::mpsc::unbounded_channel();
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
            steps: tokio::task::JoinSet::new(),
            active: HashSet::new(),
            run_cursor: None,
            waiting_cursor: None,
            step_reports,
            finished_steps,
            step_outcomes,
            capacity_exhausted: Arc::new(AtomicBool::new(false)),
            step_retries: HashMap::new(),
            #[cfg(test)]
            step_exit_gate: self.step_exit_gate,
        };
        worker.run(&mut pass, shutdown).await;
        pass.steps.abort_all();
        while pass.steps.join_next().await.is_some() {}
    }
}

struct SchedulePass {
    store: Arc<Mutex<AutomationStore>>,
    runner: ScheduledRunWorker,
    /// Owns the step tasks so shutdown can abort them; finished steps are read from
    /// `finished_steps`, never from this set.
    steps: tokio::task::JoinSet<()>,
    /// Runs with a step in flight.
    active: HashSet<RunId>,
    // Scan progress is only a fairness hint; durable Run state still owns exclusion.
    run_cursor: Option<RunId>,
    waiting_cursor: Option<ScheduleId>,
    /// Each finished step reports here before it wakes the loop, so a wake always finds
    /// the report it announces.
    step_reports: tokio::sync::mpsc::UnboundedSender<StepReport>,
    finished_steps: tokio::sync::mpsc::UnboundedReceiver<StepReport>,
    /// Moves when a step fails, or finishes while a Run waited for a free slot.
    step_outcomes: watch::Sender<u64>,
    capacity_exhausted: Arc<AtomicBool>,
    /// A Run whose step failed waits for its retry time; other Runs keep stepping. Steps
    /// fail after the pass that started them, so these retries come back through
    /// `next_due`.
    step_retries: HashMap<RunId, StepRetry>,
    /// Under test, holds each step task after it reported, before it returns.
    #[cfg(test)]
    step_exit_gate: Option<Arc<Mutex<()>>>,
}

/// What a finished step tells the pass.
struct StepReport {
    run_id: RunId,
    failed: bool,
}

/// Reports a step when dropped: as finished with its outcome, or as failed when the task
/// panicked or was aborted first. The report is sent before the wake.
struct StepReporter {
    run_id: Option<RunId>,
    failed: bool,
    reports: tokio::sync::mpsc::UnboundedSender<StepReport>,
    outcomes: watch::Sender<u64>,
    capacity_exhausted: Arc<AtomicBool>,
}

impl StepReporter {
    fn finished(mut self, failed: bool) {
        self.failed = failed;
    }
}

impl Drop for StepReporter {
    fn drop(&mut self) {
        let Some(run_id) = self.run_id.take() else {
            return;
        };
        let _ = self.reports.send(StepReport {
            run_id,
            failed: self.failed,
        });
        // "Still pending" writes nothing and wakes nothing; a failure or a freed slot
        // that a Run waits for does.
        if self.failed || self.capacity_exhausted.swap(false, Ordering::AcqRel) {
            self.outcomes
                .send_modify(|revision| *revision = revision.wrapping_add(1));
        }
    }
}

struct StepRetry {
    consecutive_failures: u32,
    retry_at: SyncTime,
}

impl SchedulePass {
    /// Takes every finished step's report. A failed, interrupted or client-unreachable
    /// step arms its Run's retry, which backs off across that Run's run of failures; a
    /// step that succeeds clears it.
    fn reap_finished_steps(&mut self, now: SyncTime) {
        while self.steps.try_join_next().is_some() {}
        while let Ok(StepReport { run_id, failed }) = self.finished_steps.try_recv() {
            self.active.remove(&run_id);
            if !failed {
                self.step_retries.remove(&run_id);
                continue;
            }
            let retry = self.step_retries.entry(run_id).or_insert(StepRetry {
                consecutive_failures: 0,
                retry_at: now,
            });
            if retry.consecutive_failures == 0 {
                tracing::warn!(
                    "scheduled Run step unavailable; retained state remains inspectable and the step retries with backoff"
                );
            }
            retry.retry_at = now.after(FAILED_STEP_BACKOFF.delay(retry.consecutive_failures));
            retry.consecutive_failures = retry.consecutive_failures.saturating_add(1);
        }
    }

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
            if self.active.len() >= MAX_ACTIVE_STEPS {
                // A step that finishes now frees a slot and wakes the loop.
                self.capacity_exhausted.store(true, Ordering::Release);
                return Ok(RunScan::AtCapacity);
            }
            self.run_cursor = Some(id.clone());
            let waiting_for_retry = self
                .step_retries
                .get(&id)
                .is_some_and(|retry| retry.retry_at > now);
            if !waiting_for_retry && !self.active.contains(&id) {
                self.spawn_step(id);
            }
        }
        if read < INVENTORY_BATCH {
            self.run_cursor = None;
            return Ok(RunScan::Complete);
        }
        Ok(RunScan::MoreRuns)
    }

    fn spawn_step(&mut self, id: RunId) {
        let runner = self.runner.clone();
        let reporter = StepReporter {
            run_id: Some(id.clone()),
            failed: true,
            reports: self.step_reports.clone(),
            outcomes: self.step_outcomes.clone(),
            capacity_exhausted: Arc::clone(&self.capacity_exhausted),
        };
        #[cfg(test)]
        let exit_gate = self.step_exit_gate.clone();
        self.active.insert(id.clone());
        self.steps.spawn(async move {
            let result =
                tokio::time::timeout(STEP_TIMEOUT, Box::pin(runner.step_reporting_client(id)))
                    .await;
            let failed = !matches!(
                result,
                Ok(Ok(crate::scheduled_run_worker::StepProgress::Stepped))
            );
            reporter.finished(failed);
            #[cfg(test)]
            if let Some(gate) = exit_gate {
                drop(gate.lock().await);
            }
        });
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
        self.reap_finished_steps(now);
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
        let retry = self
            .step_retries
            .values()
            .map(|retry| retry.retry_at)
            .filter(|retry_at| *retry_at > now)
            .min();
        Ok(match (stored, retry) {
            (Some(stored), Some(retry)) => Some(stored.min(retry)),
            (stored, retry) => stored.or(retry),
        })
    }
}

#[cfg(test)]
#[path = "schedule_timing_worker_test.rs"]
mod tests;
