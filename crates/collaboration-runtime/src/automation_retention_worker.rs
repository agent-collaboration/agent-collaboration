//! Retention for automation and interaction history, independent of native availability.
//!
//! The worker sleeps until a store commits or the first moment a retention rule allows a
//! row to go. Each pass reads every class's next due time, deletes only the classes that
//! are due, and arms one timer at the earliest due time or failed-class retry.
use crate::{BatchOutcome, WakeDrivenWorkerLoop, WorkerObservation, WorkerPass};
use automation_storage::{AutomationRetentionClass, AutomationStore};
use runtime_primitives::SyncTime;
use sqlite_storage_maintenance::FailedPassBackoff;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const INTERACTION_PRUNE_BATCH: u32 = 500;

pub struct AutomationRetentionWorker {
    store: Arc<Mutex<AutomationStore>>,
    clock: Arc<dyn router_clock::RuntimeClock>,
    interaction_broker: Option<Arc<crate::ServiceInteractionBroker>>,
    native_push_store: Option<Arc<interaction_storage::InteractionStore>>,
}

impl AutomationRetentionWorker {
    pub fn new(
        store: Arc<Mutex<AutomationStore>>,
        clock: Arc<dyn router_clock::RuntimeClock>,
    ) -> Self {
        Self {
            store,
            clock,
            interaction_broker: None,
            native_push_store: None,
        }
    }

    pub fn with_native_push_store(
        mut self,
        store: Arc<interaction_storage::InteractionStore>,
    ) -> Self {
        self.native_push_store = Some(store);
        self
    }

    pub fn with_interaction_broker(mut self, broker: Arc<crate::ServiceInteractionBroker>) -> Self {
        self.interaction_broker = Some(broker);
        self
    }

    /// Runs passes until `shutdown`. The first pass drains whatever is already due.
    pub async fn run(self, shutdown: CancellationToken) {
        let (worker_loop, mut pass) = self.into_loop().await;
        worker_loop.run(&mut pass, shutdown).await;
    }

    /// The loop, subscribed to every store whose commits can make a row deletable, and the
    /// pass it drives.
    pub(crate) async fn into_loop(self) -> (WakeDrivenWorkerLoop, RetentionPass) {
        let mut worker_loop =
            WakeDrivenWorkerLoop::new("automation_retention", Arc::clone(&self.clock))
                .wake_on(self.store.lock().await.subscribe_commits());
        if let Some(store) = &self.native_push_store {
            worker_loop = worker_loop.wake_on(store.subscribe_commits());
        }
        // Settling a pending interaction commits on the broker's own store; watch it too
        // unless it is the push store already watched.
        if let Some(broker) = &self.interaction_broker {
            let history_store = broker.interaction_store();
            let already_watched = self
                .native_push_store
                .as_ref()
                .is_some_and(|store| Arc::ptr_eq(store, &history_store));
            if !already_watched {
                worker_loop = worker_loop.wake_on(history_store.subscribe_commits());
            }
        }
        (worker_loop, self.retention_pass())
    }

    fn retention_pass(&self) -> RetentionPass {
        let mut classes: Vec<ClassSchedule> = AutomationRetentionClass::ALL
            .into_iter()
            .map(|class| ClassSchedule::new(RetentionClass::Automation(class)))
            .collect();
        if self.native_push_store.is_some() {
            classes.push(ClassSchedule::new(RetentionClass::InteractionRecords));
        }
        if self.interaction_broker.is_some() {
            classes.push(ClassSchedule::new(RetentionClass::InteractionHistory));
        }
        RetentionPass {
            store: Arc::clone(&self.store),
            interaction_broker: self.interaction_broker.clone(),
            native_push_store: self.native_push_store.clone(),
            classes,
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

/// One kind of record a retention rule prunes, with its own retry.
#[derive(Clone, Copy, Debug)]
enum RetentionClass {
    Automation(AutomationRetentionClass),
    InteractionRecords,
    InteractionHistory,
}

impl RetentionClass {
    const fn name(self) -> &'static str {
        match self {
            Self::Automation(class) => class.as_str(),
            Self::InteractionRecords => "interaction records",
            Self::InteractionHistory => "interaction history",
        }
    }

    const fn due_read(self) -> &'static str {
        match self {
            Self::Automation(AutomationRetentionClass::Events) => "retention.events.due",
            Self::Automation(AutomationRetentionClass::SettledMailboxDeliveries) => {
                "retention.mailbox_deliveries.due"
            }
            Self::Automation(AutomationRetentionClass::OperationReceipts) => {
                "retention.operation_receipts.due"
            }
            Self::Automation(AutomationRetentionClass::CompletedWorkflowRuns) => {
                "retention.workflow_runs.due"
            }
            Self::Automation(AutomationRetentionClass::TerminalWakes) => {
                "retention.terminal_wakes.due"
            }
            Self::Automation(AutomationRetentionClass::OldInstructionRevisions) => {
                "retention.instruction_revisions.due"
            }
            Self::InteractionRecords => "retention.interaction_records.due",
            Self::InteractionHistory => "retention.interaction_history.due",
        }
    }

    const fn prune_write(self) -> &'static str {
        match self {
            Self::Automation(AutomationRetentionClass::Events) => "retention.events.prune",
            Self::Automation(AutomationRetentionClass::SettledMailboxDeliveries) => {
                "retention.mailbox_deliveries.prune"
            }
            Self::Automation(AutomationRetentionClass::OperationReceipts) => {
                "retention.operation_receipts.prune"
            }
            Self::Automation(AutomationRetentionClass::CompletedWorkflowRuns) => {
                "retention.workflow_runs.prune"
            }
            Self::Automation(AutomationRetentionClass::TerminalWakes) => {
                "retention.terminal_wakes.prune"
            }
            Self::Automation(AutomationRetentionClass::OldInstructionRevisions) => {
                "retention.instruction_revisions.prune"
            }
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
    store: Arc<Mutex<AutomationStore>>,
    interaction_broker: Option<Arc<crate::ServiceInteractionBroker>>,
    native_push_store: Option<Arc<interaction_storage::InteractionStore>>,
    classes: Vec<ClassSchedule>,
}

impl RetentionPass {
    /// Reads each class's due time and prunes one batch of every class that is due. A class
    /// that fails keeps its own retry time; the others go on. Returns the rows removed.
    async fn prune_due_classes(&mut self, now_ms: i64, observation: &WorkerObservation) -> u64 {
        let mut pruned_total = 0_u64;
        let mut automation_pruned = false;
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
                    automation_pruned |=
                        pass.pruned > 0 && matches!(class, RetentionClass::Automation(_));
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
        if automation_pruned
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
        class: RetentionClass,
        now_ms: i64,
        observation: &WorkerObservation,
    ) -> Result<ClassPass, String> {
        observation.record_read(class.due_read());
        let due_ms = self.read_due(class, now_ms).await?;
        if !due_ms.is_some_and(|due_ms| due_ms <= now_ms) {
            return Ok(ClassPass { pruned: 0, due_ms });
        }
        observation.record_read(class.prune_write());
        let pruned = self.prune(class, now_ms).await?;
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

    async fn read_due(&self, class: RetentionClass, now_ms: i64) -> Result<Option<i64>, String> {
        let now = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now_ms)
            .ok_or_else(|| "retention clock is outside the supported range".to_owned())?;
        match class {
            RetentionClass::Automation(class) => class
                .due_ms(&mut *self.store.lock().await, now_ms)
                .await
                .map_err(|error| error.to_string()),
            RetentionClass::InteractionRecords => match &self.native_push_store {
                Some(store) => store
                    .push_records_prune_due(now)
                    .await
                    .map(|due| due.map(|due| due.timestamp_millis()))
                    .map_err(|error| error.to_string()),
                None => Ok(None),
            },
            RetentionClass::InteractionHistory => match &self.interaction_broker {
                Some(broker) => Ok(broker
                    .interaction_history_prune_due(now)
                    .await
                    .map(|due| due.timestamp_millis())),
                None => Ok(None),
            },
        }
    }

    async fn prune(&self, class: RetentionClass, now_ms: i64) -> Result<u64, String> {
        let now = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now_ms)
            .ok_or_else(|| "retention clock is outside the supported range".to_owned())?;
        match class {
            RetentionClass::Automation(class) => class
                .prune(&mut *self.store.lock().await, now_ms, class.max_batch())
                .await
                .map_err(|error| error.to_string()),
            RetentionClass::InteractionRecords => match &self.native_push_store {
                Some(store) => store
                    .prune_push_records(now, INTERACTION_PRUNE_BATCH)
                    .await
                    .map_err(|error| error.to_string()),
                None => Ok(0),
            },
            RetentionClass::InteractionHistory => match &self.interaction_broker {
                Some(broker) => broker
                    .prune_interaction_history(now, 500)
                    .await
                    .map_err(|error| error.to_string()),
                None => Ok(0),
            },
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
use crate::native_worker_faults as native_retention_faults;

#[cfg(test)]
#[path = "automation_retention_worker_test.rs"]
mod tests;

#[cfg(test)]
#[path = "automation_retention_worker/interaction_history_retention_tests.rs"]
mod interaction_history_retention_tests;

#[cfg(test)]
#[path = "automation_retention_worker/retention_test_harness.rs"]
mod retention_test_harness;

#[cfg(test)]
#[path = "automation_retention_worker/retention_schedule_tests.rs"]
mod retention_schedule_tests;

#[cfg(test)]
#[path = "automation_retention_worker/retention_fault_tests.rs"]
mod retention_fault_tests;
