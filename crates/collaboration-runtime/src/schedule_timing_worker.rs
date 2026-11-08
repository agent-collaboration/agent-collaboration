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
    #[cfg(test)]
    hold_full_table: bool,
    #[cfg(test)]
    reap_hold: Option<ReapHold>,
}

/// Held reports a test releases between a pass taking events and taking reports.
#[cfg(test)]
struct ReapHold {
    held: tokio::sync::OwnedMutexGuard<()>,
    /// Set by the test just before it publishes the event.
    armed: Arc<std::sync::atomic::AtomicBool>,
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
            #[cfg(test)]
            hold_full_table: false,
            #[cfg(test)]
            reap_hold: None,
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

    /// Holds every step task after it enqueued its report and before it reads the event
    /// generation, until the test releases the gate.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn hold_reports_before_generation(mut self, gate: Arc<Mutex<()>>) -> Self {
        self.step_gate = Some(crate::keyed_worker_tasks::TaskGate::new(
            crate::keyed_worker_tasks::TaskGatePoint::BetweenReportAndGeneration,
            gate,
        ));
        self
    }

    /// The first pass after the test sets `armed` releases `held` between taking its
    /// events and taking its queued reports, and waits there until a report is queued.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn release_between_events_and_reap(
        mut self,
        held: tokio::sync::OwnedMutexGuard<()>,
        armed: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        self.reap_hold = Some(ReapHold { held, armed });
        self
    }

    /// As [`Self::hold_step_reports`], counting in `arrivals` each step held.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn hold_step_reports_counting(
        mut self,
        gate: Arc<Mutex<()>>,
        arrivals: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        self.step_gate = Some(crate::keyed_worker_tasks::TaskGate::counting(
            crate::keyed_worker_tasks::TaskGatePoint::BeforeReport,
            gate,
            arrivals,
        ));
        self
    }

    /// The first time every step slot is taken, holds the scheduler before it waits for
    /// a slot until all of those steps have queued their reports.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn hold_full_table_until_reports_queued(mut self) -> Self {
        self.hold_full_table = true;
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
        // The pass tells an event from its own steps' reports through its own copies.
        let mut events = vec![commits.clone(), configuration.clone()];
        events.extend(self.completion_sources.iter().cloned());
        #[cfg_attr(not(test), expect(unused_mut, reason = "only tests hold step tasks"))]
        let (mut steps, step_wakes) = KeyedWorkerTasks::new(
            MAX_ACTIVE_STEPS,
            step_commits,
            "scheduled Run step unavailable; retained state remains inspectable and the step retries with backoff",
        );
        #[cfg(test)]
        steps.hold(self.step_gate);
        #[cfg(test)]
        if self.hold_full_table {
            steps.hold_full_table_until_reports_queued();
        }
        #[cfg(any(test, feature = "test-support"))]
        self.observation.track_running_tasks(steps.running_tasks());
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
            events,
            sweep_owed: false,
            armed: None,
            #[cfg(test)]
            reap_hold: self.reap_hold,
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
    /// Automation commits, configuration changes and completions: anything that may
    /// concern a Run, unlike a step's report or a freed slot.
    events: Vec<watch::Receiver<u64>>,
    /// An event, or the report of a step an event overtook, arrived during a sweep that
    /// may already have passed the Run it concerns: another full sweep follows this one.
    sweep_owed: bool,
    /// The due time this scheduler last armed. A wake at or after it is an event: what
    /// came due may be behind the scan, and next due reads only later times.
    armed: Option<SyncTime>,
    #[cfg(test)]
    reap_hold: Option<ReapHold>,
    // Scan progress is only a fairness hint; durable Run state still owns exclusion.
    run_cursor: Option<RunId>,
    waiting_cursor: Option<ScheduleId>,
}

impl SchedulePass {
    /// Marks every event seen and reports whether any arrived since the last batch: a
    /// commit, a configuration change, a completion, or the armed due time passing.
    fn take_events(&mut self, now: SyncTime) -> bool {
        let mut arrived = self.armed.take_if(|due| *due <= now).is_some();
        for event in &mut self.events {
            arrived |= event.has_changed().unwrap_or(false);
            event.borrow_and_update();
        }
        arrived
    }

    /// Releases the held reports between taking events and taking reports, once armed.
    #[cfg(test)]
    async fn release_held_reports(&mut self) {
        let armed = self
            .reap_hold
            .as_ref()
            .is_some_and(|hold| hold.armed.load(std::sync::atomic::Ordering::SeqCst));
        let Some(hold) = self.reap_hold.take_if(|_| armed) else {
            return;
        };
        drop(hold.held);
        while self.steps.reports_queued() == 0 {
            tokio::task::yield_now().await;
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
            if self.steps.at_capacity(now).await {
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
        // Events are taken before reports: a report queued before this point is taken
        // and its Run stepped by this sweep; one queued after sees the newer generation
        // and asks for its own sweep.
        let event = self.take_events(now);
        if event {
            self.steps.event_arrived();
            self.sweep_owed = true;
        }
        #[cfg(test)]
        self.release_held_reports().await;
        self.steps.reap(now);
        if self.steps.take_sweep_request() {
            self.sweep_owed = true;
        }
        if self.run_cursor.is_none() {
            // This pass begins a full sweep from the first Run.
            self.sweep_owed = false;
        }
        let now_ms = now.unix_millis();
        let due = self.enqueue_due(now_ms, observation).await?;
        let waiting = self.admit_waiting(now_ms, observation).await?;
        let runs = self.start_observable_steps(now, observation).await?;
        // A sweep that reached the last Run with another owed starts again from the first.
        let more = due == INVENTORY_BATCH
            || waiting == INVENTORY_BATCH
            || matches!(runs, RunScan::MoreRuns)
            || (matches!(runs, RunScan::Complete) && self.sweep_owed);
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
        self.armed = match (stored, self.steps.next_retry(now)) {
            (Some(stored), Some(retry)) => Some(stored.min(retry)),
            (stored, retry) => stored.or(retry),
        };
        Ok(self.armed)
    }
}

#[cfg(test)]
#[path = "schedule_timing_worker_test.rs"]
mod tests;
