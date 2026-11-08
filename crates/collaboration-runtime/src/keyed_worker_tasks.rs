//! The tasks a worker runs per key (a Run's step, a delivery's dispatch) and what each
//! tells the worker's loop when it finishes.
//!
//! A finished task reports its key and outcome on a channel the loop drains, and only
//! then wakes the loop, so a wake always finds what it announces. Progress (the store's
//! commit counter moved during the task), a failure, or a slot freed while a key waited
//! for one wakes the loop; a task that changed nothing stays silent. A key whose task
//! failed waits for its retry time, 1 s doubling to 30 s across its run of failures, and
//! logs once per run.
use runtime_primitives::{Backoff, SyncTime};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    hash::Hash,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch};

/// A failed task's key retries 1 s, doubling to 30 s, until a task for it succeeds.
const FAILED_TASK_BACKOFF: Backoff = Backoff {
    initial: Duration::from_secs(1),
    max: Duration::from_secs(30),
    factor: 2,
};

/// What one finished task achieved, as the loop needs to know it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TaskOutcome {
    /// It changed rows: the loop runs another pass for the key's next step.
    Progress,
    /// It changed nothing, such as a turn still running: nothing to wake for.
    Pending,
    /// It failed or could not reach its client: the key retries on its backoff.
    Failed,
}

struct TaskReport<TKey> {
    key: TKey,
    outcome: TaskOutcome,
}

struct TaskRetry {
    consecutive_failures: u32,
    retry_at: SyncTime,
}

/// Reports a task when dropped: as finished with its outcome, or as failed when the task
/// panicked or was aborted first. The report is sent before the wake.
struct TaskReporter<TKey> {
    key: Option<TKey>,
    outcome: TaskOutcome,
    reports: mpsc::UnboundedSender<TaskReport<TKey>>,
    wakes: watch::Sender<u64>,
    capacity_exhausted: Arc<AtomicBool>,
}

impl<TKey> TaskReporter<TKey> {
    fn finished(mut self, outcome: TaskOutcome) {
        self.outcome = outcome;
    }
}

impl<TKey> Drop for TaskReporter<TKey> {
    fn drop(&mut self) {
        let Some(key) = self.key.take() else {
            return;
        };
        let _ = self.reports.send(TaskReport {
            key,
            outcome: self.outcome,
        });
        if self.outcome != TaskOutcome::Pending
            || self.capacity_exhausted.swap(false, Ordering::AcqRel)
        {
            self.wakes
                .send_modify(|revision| *revision = revision.wrapping_add(1));
        }
    }
}

pub(crate) struct KeyedWorkerTasks<TKey> {
    /// Owns the tasks so shutdown can abort them; finished tasks are read from
    /// `finished`, never from this set.
    tasks: tokio::task::JoinSet<()>,
    active: HashSet<TKey>,
    capacity: usize,
    reports: mpsc::UnboundedSender<TaskReport<TKey>>,
    finished: mpsc::UnboundedReceiver<TaskReport<TKey>>,
    wakes: watch::Sender<u64>,
    capacity_exhausted: Arc<AtomicBool>,
    /// Each task compares this commit counter before and after its work.
    commits: watch::Receiver<u64>,
    retries: HashMap<TKey, TaskRetry>,
    failure_warning: &'static str,
    #[cfg(test)]
    gate: Option<TaskGate>,
}

impl<TKey: Clone + Eq + Hash + Send + 'static> KeyedWorkerTasks<TKey> {
    /// `commits` is a fresh subscription to the store the tasks write. The returned
    /// receiver is the wake source the loop subscribes to.
    pub(crate) fn new(
        capacity: usize,
        commits: watch::Receiver<u64>,
        failure_warning: &'static str,
    ) -> (Self, watch::Receiver<u64>) {
        let (reports, finished) = mpsc::unbounded_channel();
        let (wakes, wake_source) = watch::channel(0);
        (
            Self {
                tasks: tokio::task::JoinSet::new(),
                active: HashSet::new(),
                capacity,
                reports,
                finished,
                wakes,
                capacity_exhausted: Arc::new(AtomicBool::new(false)),
                commits,
                retries: HashMap::new(),
                failure_warning,
                #[cfg(test)]
                gate: None,
            },
            wake_source,
        )
    }

    /// Takes every finished task's report. A failure arms its key's retry; a task that
    /// succeeds clears it.
    pub(crate) fn reap(&mut self, now: SyncTime) {
        while self.tasks.try_join_next().is_some() {}
        while let Ok(TaskReport { key, outcome }) = self.finished.try_recv() {
            self.active.remove(&key);
            if outcome != TaskOutcome::Failed {
                self.retries.remove(&key);
                continue;
            }
            let retry = self.retries.entry(key).or_insert(TaskRetry {
                consecutive_failures: 0,
                retry_at: now,
            });
            if retry.consecutive_failures == 0 {
                tracing::warn!("{}", self.failure_warning);
            }
            retry.retry_at = now.after(FAILED_TASK_BACKOFF.delay(retry.consecutive_failures));
            retry.consecutive_failures = retry.consecutive_failures.saturating_add(1);
        }
    }

    /// No slot is free; a task that finishes now wakes the loop.
    pub(crate) fn at_capacity(&self) -> bool {
        let full = self.active.len() >= self.capacity;
        if full {
            self.capacity_exhausted.store(true, Ordering::Release);
        }
        full
    }

    /// A task for this key is running, or its retry time has not come.
    pub(crate) fn is_busy(&self, key: &TKey, now: SyncTime) -> bool {
        self.active.contains(key)
            || self
                .retries
                .get(key)
                .is_some_and(|retry| retry.retry_at > now)
    }

    /// The earliest retry time after `now`.
    pub(crate) fn next_retry(&self, now: SyncTime) -> Option<SyncTime> {
        self.retries
            .values()
            .map(|retry| retry.retry_at)
            .filter(|retry_at| *retry_at > now)
            .min()
    }

    /// Runs `work` for `key`. It resolves to whether it succeeded; whether it made
    /// progress comes from the commit counter. A commit by anyone during the work also
    /// counts, costing at most one extra pass.
    pub(crate) fn spawn(&mut self, key: TKey, work: impl Future<Output = bool> + Send + 'static) {
        let reporter = TaskReporter {
            key: Some(key.clone()),
            outcome: TaskOutcome::Failed,
            reports: self.reports.clone(),
            wakes: self.wakes.clone(),
            capacity_exhausted: Arc::clone(&self.capacity_exhausted),
        };
        let mut commits = self.commits.clone();
        commits.borrow_and_update();
        #[cfg(test)]
        let gate = self.gate.as_ref().map(TaskGate::shared);
        self.active.insert(key);
        self.tasks.spawn(async move {
            let outcome = if !work.await {
                TaskOutcome::Failed
            } else if commits.has_changed().unwrap_or(false) {
                TaskOutcome::Progress
            } else {
                TaskOutcome::Pending
            };
            #[cfg(test)]
            if let Some(gate) = &gate {
                gate.pass_at(TaskGatePoint::BeforeReport).await;
            }
            reporter.finished(outcome);
            #[cfg(test)]
            if let Some(gate) = &gate {
                gate.pass_at(TaskGatePoint::AfterReport).await;
            }
        });
    }

    pub(crate) async fn shut_down(&mut self) {
        self.tasks.abort_all();
        while self.tasks.join_next().await.is_some() {}
    }
}

/// Where a test holds every task: before it reports, or after.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskGatePoint {
    BeforeReport,
    AfterReport,
}

#[cfg(test)]
pub(crate) struct TaskGate {
    point: TaskGatePoint,
    gate: Arc<tokio::sync::Mutex<()>>,
}

#[cfg(test)]
impl TaskGate {
    pub(crate) fn new(point: TaskGatePoint, gate: Arc<tokio::sync::Mutex<()>>) -> Self {
        Self { point, gate }
    }

    fn shared(&self) -> Self {
        Self {
            point: self.point,
            gate: Arc::clone(&self.gate),
        }
    }

    async fn pass_at(&self, point: TaskGatePoint) {
        if self.point == point {
            drop(self.gate.lock().await);
        }
    }
}

#[cfg(test)]
impl<TKey> KeyedWorkerTasks<TKey> {
    pub(crate) fn hold(&mut self, gate: Option<TaskGate>) {
        self.gate = gate;
    }
}
