//! Retention for interaction records and interaction history, independent of native availability.
//!
//! Both classes live in the approval broker's interaction store. The worker sleeps until that
//! store commits or the first moment a retention rule allows a row to go. Each pass reads every
//! class's next due time, deletes only the classes that are due, and arms one timer at the
//! earliest due time or failed-class retry.
use crate::{BatchOutcome, WakeDrivenWorkerLoop, WorkerObservation, WorkerPass};
use runtime_primitives::SyncTime;
use sqlite_storage_maintenance::FailedPassBackoff;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Rows one pass may delete per class.
const PUSH_RECORD_PRUNE_BATCH: u32 = 500;
const HISTORY_PRUNE_BATCH: usize = 500;

pub struct InteractionRetentionWorker {
    broker: Arc<crate::ServiceInteractionBroker>,
    clock: Arc<dyn router_clock::RuntimeClock>,
}

impl InteractionRetentionWorker {
    /// Retains the broker's history and the push records of the broker's interaction store.
    pub fn new(
        broker: Arc<crate::ServiceInteractionBroker>,
        clock: Arc<dyn router_clock::RuntimeClock>,
    ) -> Self {
        Self { broker, clock }
    }

    /// Runs passes until `shutdown`. The first pass drains whatever is already due.
    pub async fn run(self, shutdown: CancellationToken) {
        let (worker_loop, mut pass) = self.into_loop();
        worker_loop.run(&mut pass, shutdown).await;
    }

    /// The loop, woken by every commit of the interaction store, and the pass it drives.
    pub(crate) fn into_loop(self) -> (WakeDrivenWorkerLoop, RetentionPass) {
        let worker_loop =
            WakeDrivenWorkerLoop::new("interaction_retention", Arc::clone(&self.clock))
                .wake_on(self.broker.interaction_store().subscribe_commits());
        (worker_loop, self.retention_pass())
    }

    fn retention_pass(&self) -> RetentionPass {
        RetentionPass {
            broker: Arc::clone(&self.broker),
            classes: RetentionClass::ALL
                .into_iter()
                .map(ClassSchedule::new)
                .collect(),
        }
    }

    /// Prunes one batch of every class that is due at `now_ms`, as one pass does. Test-only:
    /// the Host runs the worker through [`Self::run`].
    #[cfg(any(test, feature = "test-support"))]
    pub async fn prune_batch(&self, now_ms: i64) -> u64 {
        self.retention_pass()
            .prune_due_classes(now_ms, &WorkerObservation::default())
            .await
    }
}

/// One kind of record a retention rule prunes, with its own retry.
#[derive(Clone, Copy, Debug)]
enum RetentionClass {
    /// Settled push records: messages, notices and their answers.
    InteractionRecords,
    /// The broker's settled approval and question history.
    InteractionHistory,
}

impl RetentionClass {
    const ALL: [Self; 2] = [Self::InteractionRecords, Self::InteractionHistory];

    const fn name(self) -> &'static str {
        match self {
            Self::InteractionRecords => "interaction records",
            Self::InteractionHistory => "interaction history",
        }
    }

    const fn due_read(self) -> &'static str {
        match self {
            Self::InteractionRecords => "retention.interaction_records.due",
            Self::InteractionHistory => "retention.interaction_history.due",
        }
    }

    const fn prune_write(self) -> &'static str {
        match self {
            Self::InteractionRecords => "retention.interaction_records.prune",
            Self::InteractionHistory => "retention.interaction_history.prune",
        }
    }
}

/// A class's next due time and its own failure retry, so one class's success never clears
/// another's retry.
struct ClassSchedule {
    class: RetentionClass,
    due_ms: Option<i64>,
    retry_at_ms: Option<i64>,
    failures: FailedPassBackoff,
}

impl ClassSchedule {
    fn new(class: RetentionClass) -> Self {
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
    broker: Arc<crate::ServiceInteractionBroker>,
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
                        tracing::info!(retention_class = class.name(), "retention prune recovered");
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
                            retention_class = class.name(),
                            "retention prune failed; it retries with backoff"
                        );
                    }
                    let delay_ms = i64::try_from(retry.delay.as_millis()).unwrap_or(i64::MAX);
                    schedule.retry_at_ms = Some(now_ms.saturating_add(delay_ms));
                    schedule.due_ms = None;
                }
            }
        }
        pruned_total
    }

    /// Reads `class`'s due time and, only when it has passed, prunes one batch. A class
    /// that pruned reports itself due now, so the next batch reads it again.
    async fn run_class(
        &self,
        class: RetentionClass,
        now_ms: i64,
        observation: &WorkerObservation,
    ) -> Result<ClassPass, String> {
        let now = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now_ms)
            .ok_or_else(|| "retention clock is outside the supported range".to_owned())?;
        observation.record_read(class.due_read());
        let due_ms = self.read_due(class, now).await?;
        if !due_ms.is_some_and(|due_ms| due_ms <= now_ms) {
            return Ok(ClassPass { pruned: 0, due_ms });
        }
        observation.record_read(class.prune_write());
        let pruned = self.prune(class, now).await?;
        if pruned == 0 {
            // The due read and the DELETE share one rule; a due class that deletes nothing
            // retries on its backoff instead of re-arming a timer in the past.
            return Err(format!(
                "{} was due but its prune removed nothing",
                class.name()
            ));
        }
        Ok(ClassPass {
            pruned,
            due_ms: Some(now_ms),
        })
    }

    async fn read_due(
        &self,
        class: RetentionClass,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<i64>, String> {
        let due = match class {
            RetentionClass::InteractionRecords => self
                .broker
                .interaction_store()
                .push_records_prune_due(now)
                .await
                .map_err(|error| error.to_string())?,
            RetentionClass::InteractionHistory => self
                .broker
                .interaction_history_prune_due(now)
                .await
                .map_err(|error| error.to_string())?,
        };
        Ok(due.map(|due| due.timestamp_millis()))
    }

    async fn prune(
        &self,
        class: RetentionClass,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, String> {
        match class {
            RetentionClass::InteractionRecords => self
                .broker
                .interaction_store()
                .prune_push_records(now, PUSH_RECORD_PRUNE_BATCH)
                .await
                .map_err(|error| error.to_string()),
            RetentionClass::InteractionHistory => self
                .broker
                .prune_interaction_history(now, HISTORY_PRUNE_BATCH)
                .await
                .map_err(|error| error.to_string()),
        }
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
#[path = "interaction_retention_worker_test.rs"]
mod tests;

#[cfg(test)]
#[path = "interaction_retention_worker/retention_test_harness.rs"]
pub(crate) mod retention_test_harness;

#[cfg(test)]
#[path = "interaction_retention_worker/interaction_history_retention_tests.rs"]
pub(crate) mod interaction_history_retention_tests;

#[cfg(test)]
#[path = "interaction_retention_worker/retention_schedule_tests.rs"]
mod retention_schedule_tests;

#[cfg(test)]
#[path = "interaction_retention_worker/retention_fault_tests.rs"]
mod retention_fault_tests;
