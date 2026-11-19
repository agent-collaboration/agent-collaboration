//! Retention for automation history, independent of native availability. Interaction records
//! and history are retained by [`crate::InteractionRetentionWorker`].
//!
//! The worker sleeps until the store commits or the first moment a retention rule allows a
//! row to go. Each pass reads every class's next due time, deletes only the classes that
//! are due, and arms one timer at the earliest due time or failed-class retry.
use crate::{BatchOutcome, WakeDrivenWorkerLoop, WorkerObservation, WorkerPass};
use automation_storage::{AutomationRetentionClass, AutomationStore};
use runtime_primitives::SyncTime;
use sqlite_storage_maintenance::FailedPassBackoff;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub struct AutomationRetentionWorker {
    store: Arc<Mutex<AutomationStore>>,
    clock: Arc<dyn router_clock::RuntimeClock>,
}

impl AutomationRetentionWorker {
    pub fn new(
        store: Arc<Mutex<AutomationStore>>,
        clock: Arc<dyn router_clock::RuntimeClock>,
    ) -> Self {
        Self { store, clock }
    }

    /// Runs passes until `shutdown`. The first pass drains whatever is already due.
    pub async fn run(self, shutdown: CancellationToken) {
        let (worker_loop, mut pass) = self.into_loop().await;
        worker_loop.run(&mut pass, shutdown).await;
    }

    /// The loop, subscribed to the store whose commits can make a row deletable, and the pass
    /// it drives.
    pub(crate) async fn into_loop(self) -> (WakeDrivenWorkerLoop, RetentionPass) {
        let worker_loop =
            WakeDrivenWorkerLoop::new("automation_retention", Arc::clone(&self.clock))
                .wake_on(self.store.lock().await.subscribe_commits());
        (worker_loop, self.retention_pass())
    }

    fn retention_pass(&self) -> RetentionPass {
        RetentionPass {
            store: Arc::clone(&self.store),
            classes: AutomationRetentionClass::ALL
                .into_iter()
                .map(ClassSchedule::new)
                .collect(),
        }
    }

    /// Prunes one batch of every class that is due at `now_ms`, as one pass does.
    pub async fn prune_batch(&self, now_ms: i64) -> Result<u64, automation_storage::StorageError> {
        let mut pass = self.retention_pass();
        Ok(pass
            .prune_due_classes(now_ms, &WorkerObservation::default())
            .await)
    }
}

const fn due_read(class: AutomationRetentionClass) -> &'static str {
    match class {
        AutomationRetentionClass::Events => "retention.events.due",
        AutomationRetentionClass::SettledMailboxDeliveries => "retention.mailbox_deliveries.due",
        AutomationRetentionClass::OperationReceipts => "retention.operation_receipts.due",
        AutomationRetentionClass::CompletedWorkflowRuns => "retention.workflow_runs.due",
        AutomationRetentionClass::TerminalWakes => "retention.terminal_wakes.due",
        AutomationRetentionClass::OldInstructionRevisions => "retention.instruction_revisions.due",
    }
}

const fn prune_write(class: AutomationRetentionClass) -> &'static str {
    match class {
        AutomationRetentionClass::Events => "retention.events.prune",
        AutomationRetentionClass::SettledMailboxDeliveries => "retention.mailbox_deliveries.prune",
        AutomationRetentionClass::OperationReceipts => "retention.operation_receipts.prune",
        AutomationRetentionClass::CompletedWorkflowRuns => "retention.workflow_runs.prune",
        AutomationRetentionClass::TerminalWakes => "retention.terminal_wakes.prune",
        AutomationRetentionClass::OldInstructionRevisions => {
            "retention.instruction_revisions.prune"
        }
    }
}

/// A class's next due time and its own failure retry, so one class's success never clears
/// another's retry.
struct ClassSchedule {
    class: AutomationRetentionClass,
    due_ms: Option<i64>,
    retry_at_ms: Option<i64>,
    failures: FailedPassBackoff,
}

impl ClassSchedule {
    fn new(class: AutomationRetentionClass) -> Self {
        Self {
            class,
            due_ms: None,
            retry_at_ms: None,
            failures: FailedPassBackoff::default(),
        }
    }
}

/// What one class did in a pass.
struct ClassPass {
    pruned: u64,
    due_ms: Option<i64>,
}

pub(crate) struct RetentionPass {
    store: Arc<Mutex<AutomationStore>>,
    classes: Vec<ClassSchedule>,
}

impl RetentionPass {
    /// Reads each class's due time and prunes one batch of every class that is due. A class
    /// that fails keeps its own retry time; the others go on. Returns the rows removed.
    async fn prune_due_classes(&mut self, now_ms: i64, observation: &WorkerObservation) -> u64 {
        let mut pruned_total = 0_u64;
        for index in 0..self.classes.len() {
            let Some(schedule) = self.classes.get(index) else {
                continue;
            };
            if schedule
                .retry_at_ms
                .is_some_and(|retry_at| retry_at > now_ms)
            {
                continue;
            }
            let class = schedule.class;
            let result = self.run_class(class, now_ms, observation).await;
            let Some(schedule) = self.classes.get_mut(index) else {
                continue;
            };
            match result {
                Ok(pass) => {
                    if schedule.failures.record_success() {
                        tracing::info!(
                            retention_class = class.as_str(),
                            "retention prune recovered"
                        );
                    }
                    schedule.retry_at_ms = None;
                    schedule.due_ms = pass.due_ms;
                    pruned_total = pruned_total.saturating_add(pass.pruned);
                }
                Err(error) => {
                    let retry = schedule.failures.record_failure();
                    if retry.starts_run {
                        tracing::warn!(
                            %error,
                            retention_class = class.as_str(),
                            "retention prune failed; it retries with backoff"
                        );
                    }
                    let delay_ms = i64::try_from(retry.delay.as_millis()).unwrap_or(i64::MAX);
                    schedule.retry_at_ms = Some(now_ms.saturating_add(delay_ms));
                    schedule.due_ms = None;
                }
            }
        }
        if pruned_total > 0
            && let Err(error) = self.store.lock().await.incremental_vacuum_step().await
        {
            tracing::warn!(
                %error,
                "automation incremental-vacuum step failed; the next prune will retry"
            );
        }
        pruned_total
    }

    /// Reads `class`'s due time and, only when it has passed, prunes one batch. A class
    /// that pruned reports itself due now, so the next batch reads it again.
    async fn run_class(
        &self,
        class: AutomationRetentionClass,
        now_ms: i64,
        observation: &WorkerObservation,
    ) -> Result<ClassPass, String> {
        observation.record_read(due_read(class));
        let due_ms = class
            .due_ms(&mut *self.store.lock().await, now_ms)
            .await
            .map_err(|error| error.to_string())?;
        if !due_ms.is_some_and(|due_ms| due_ms <= now_ms) {
            return Ok(ClassPass { pruned: 0, due_ms });
        }
        observation.record_read(prune_write(class));
        let pruned = class
            .prune(&mut *self.store.lock().await, now_ms, class.max_batch())
            .await
            .map_err(|error| error.to_string())?;
        if pruned == 0 {
            // The due read and the DELETE share one rule; a due class that deletes nothing
            // retries on its backoff instead of re-arming a timer in the past.
            return Err(format!(
                "{} was due but its prune removed nothing",
                class.as_str()
            ));
        }
        Ok(ClassPass {
            pruned,
            due_ms: Some(now_ms),
        })
    }
}

impl WorkerPass for RetentionPass {
    type Error = std::convert::Infallible;

    async fn drain_batch(
        &mut self,
        now: SyncTime,
        observation: &WorkerObservation,
    ) -> Result<BatchOutcome, Self::Error> {
        let pruned = self.prune_due_classes(now.unix_millis(), observation).await;
        Ok(if pruned > 0 {
            BatchOutcome::MoreWork
        } else {
            BatchOutcome::Drained
        })
    }

    async fn next_due(
        &mut self,
        _now: SyncTime,
        _observation: &WorkerObservation,
    ) -> Result<Option<SyncTime>, Self::Error> {
        Ok(self
            .classes
            .iter()
            .flat_map(|schedule| [schedule.due_ms, schedule.retry_at_ms])
            .flatten()
            .min()
            .map(SyncTime::from_unix_millis))
    }
}

#[cfg(test)]
#[path = "automation_retention_worker_test.rs"]
mod tests;

#[cfg(test)]
#[path = "automation_retention_worker/retention_test_harness.rs"]
mod retention_test_harness;

#[cfg(test)]
#[path = "automation_retention_worker/retention_schedule_tests.rs"]
mod retention_schedule_tests;

#[cfg(test)]
#[path = "automation_retention_worker/retention_fault_tests.rs"]
mod retention_fault_tests;
