//! The tasks a worker runs per key (a Run's step, a delivery's dispatch) and what each
//! tells the worker's loop when it finishes.
//!
//! A finished task reports its key and outcome on a channel the loop drains, and only
//! then wakes the loop, so a wake always finds what it announces. Progress (the store's
//! commit counter moved during the task), a failure, a slot freed while the loop waited
//! for one, or an event that arrived after the task started wakes the loop; a task that
//! changed nothing and that no event may have overtaken stays silent. A key whose task
//! failed waits for its retry time, 1 s doubling to 30 s across its run of failures, and
//! logs once per run.
//!
//! Events are counted by a generation the loop advances whenever it takes one. A task
//! records the generation it started at; when it reports into a newer one, whatever it
//! observed may predate that event, so it asks the loop for another full sweep.
//!
//! Two orders keep a report from being lost. A reporter enqueues its report before it
//! reads the generation and whether the loop waits for a slot; the loop advances the
//! generation, or marks that it waits, before it looks for queued reports. Either the
//! reporter sees the loop's mark or the loop sees the report.
use runtime_primitives::{Backoff, SyncTime};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    hash::Hash,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering, fence},
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

/// What a task's report shares with the loop.
struct ReportChannels<TKey> {
    reports: mpsc::UnboundedSender<TaskReport<TKey>>,
    wakes: watch::Sender<u64>,
    capacity_exhausted: Arc<AtomicBool>,
    event_generation: Arc<AtomicU64>,
    sweep_requested: Arc<AtomicBool>,
    /// Counts tasks that have not reported, for tests deciding the worker is idle.
    #[cfg(any(test, feature = "test-support"))]
    running: Arc<std::sync::atomic::AtomicUsize>,
}

/// Reports a task: as finished with its outcome, or as failed when the task panicked or
/// was aborted first. The report is enqueued before anything it tells the loop is read.
struct TaskReporter<TKey> {
    key: Option<TKey>,
    outcome: TaskOutcome,
    /// The event generation the task started at.
    started_at: u64,
    channels: ReportChannels<TKey>,
    /// The report is enqueued and the loop not yet told.
    enqueued: bool,
}

impl<TKey> TaskReporter<TKey> {
    /// Enqueues the report; false when it already was.
    fn enqueue(&mut self) -> bool {
        let Some(key) = self.key.take() else {
            return false;
        };
        let _ = self.channels.reports.send(TaskReport {
            key,
            outcome: self.outcome,
        });
        self.enqueued = true;
        true
    }

    /// Tells the loop what the enqueued report needs: a wake for progress, a failure, a
    /// freed slot it waits for, or an event that may have overtaken the task.
    fn notify(&mut self, generation: u64) {
        if !std::mem::take(&mut self.enqueued) {
            return;
        }
        let overtaken = generation > self.started_at;
        if overtaken {
            self.channels.sweep_requested.store(true, Ordering::SeqCst);
        }
        let slot_awaited = self
            .channels
            .capacity_exhausted
            .swap(false, Ordering::SeqCst);
        if self.outcome != TaskOutcome::Pending || overtaken || slot_awaited {
            self.channels
                .wakes
                .send_modify(|revision| *revision = revision.wrapping_add(1));
        }
        // Counted down only once the report and its wake are published.
        #[cfg(any(test, feature = "test-support"))]
        self.channels.running.fetch_sub(1, Ordering::SeqCst);
    }

    fn generation(&self) -> u64 {
        // The report is queued before the generation is read.
        fence(Ordering::SeqCst);
        self.channels.event_generation.load(Ordering::SeqCst)
    }

    #[cfg_attr(
        not(test),
        expect(
            clippy::unused_async,
            reason = "tests hold the reporter here, between enqueuing and reading the generation"
        )
    )]
    async fn publish(mut self, outcome: TaskOutcome, #[cfg(test)] gate: Option<&TaskGate>) {
        self.outcome = outcome;
        self.enqueue();
        #[cfg(test)]
        if let Some(gate) = gate {
            gate.pass_at(TaskGatePoint::BetweenReportAndGeneration)
                .await;
        }
        let generation = self.generation();
        self.notify(generation);
    }
}

impl<TKey> Drop for TaskReporter<TKey> {
    fn drop(&mut self) {
        // A task that panicked or was aborted reports here, even halfway through.
        self.enqueue();
        let generation = self.generation();
        self.notify(generation);
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
    event_generation: Arc<AtomicU64>,
    sweep_requested: Arc<AtomicBool>,
    #[cfg(any(test, feature = "test-support"))]
    running: Arc<std::sync::atomic::AtomicUsize>,
    /// Each task compares this commit counter before and after its work.
    commits: watch::Receiver<u64>,
    retries: HashMap<TKey, TaskRetry>,
    failure_warning: &'static str,
    #[cfg(test)]
    gate: Option<TaskGate>,
    /// Holds the loop at a full table, before it waits for a slot, until every slot's
    /// report is queued.
    #[cfg(test)]
    hold_until_reports_queued: bool,
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
                event_generation: Arc::new(AtomicU64::new(0)),
                sweep_requested: Arc::new(AtomicBool::new(false)),
                #[cfg(any(test, feature = "test-support"))]
                running: Arc::default(),
                commits,
                retries: HashMap::new(),
                failure_warning,
                #[cfg(test)]
                gate: None,
                #[cfg(test)]
                hold_until_reports_queued: false,
            },
            wake_source,
        )
    }

    /// The loop took an event: every task in flight may have observed what came before
    /// it. Call before taking the queued reports.
    pub(crate) fn event_arrived(&self) {
        self.event_generation.fetch_add(1, Ordering::SeqCst);
        fence(Ordering::SeqCst);
    }

    /// Whether a task an event overtook has reported since the last call: the loop owes
    /// another full sweep.
    pub(crate) fn take_sweep_request(&self) -> bool {
        self.sweep_requested.swap(false, Ordering::SeqCst)
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

    /// No slot is free; a task that finishes now wakes the loop. The loop marks that it
    /// waits before it looks for reports already queued, and takes them if there are
    /// any, so a report enqueued just before the mark is not left unread.
    #[cfg_attr(
        not(test),
        expect(
            clippy::unused_async,
            reason = "tests hold the loop here, before it marks the wait, to force a report first"
        )
    )]
    pub(crate) async fn at_capacity(&mut self, now: SyncTime) -> bool {
        if self.active.len() < self.capacity {
            return false;
        }
        #[cfg(test)]
        self.wait_for_every_report().await;
        self.capacity_exhausted.store(true, Ordering::SeqCst);
        fence(Ordering::SeqCst);
        if self.finished.is_empty() {
            return true;
        }
        self.reap(now);
        let full = self.active.len() >= self.capacity;
        if !full {
            self.capacity_exhausted.store(false, Ordering::SeqCst);
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

    /// The count of tasks that have not reported, for tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn running_tasks(&self) -> Arc<std::sync::atomic::AtomicUsize> {
        Arc::clone(&self.running)
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
            started_at: self.event_generation.load(Ordering::SeqCst),
            channels: ReportChannels {
                reports: self.reports.clone(),
                wakes: self.wakes.clone(),
                capacity_exhausted: Arc::clone(&self.capacity_exhausted),
                event_generation: Arc::clone(&self.event_generation),
                sweep_requested: Arc::clone(&self.sweep_requested),
                #[cfg(any(test, feature = "test-support"))]
                running: Arc::clone(&self.running),
            },
            enqueued: false,
        };
        let mut commits = self.commits.clone();
        commits.borrow_and_update();
        #[cfg(any(test, feature = "test-support"))]
        self.running.fetch_add(1, Ordering::SeqCst);
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
            #[cfg(test)]
            reporter.publish(outcome, gate.as_ref()).await;
            #[cfg(not(test))]
            reporter.publish(outcome).await;
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

/// Where a test holds every task: before it reports, between enqueuing its report and
/// reading the event generation, or after it reported.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskGatePoint {
    BeforeReport,
    BetweenReportAndGeneration,
    AfterReport,
}

#[cfg(test)]
pub(crate) struct TaskGate {
    point: TaskGatePoint,
    gate: Arc<tokio::sync::Mutex<()>>,
    /// Counts the tasks that reached the gate.
    arrivals: Arc<std::sync::atomic::AtomicUsize>,
}

#[cfg(test)]
impl TaskGate {
    pub(crate) fn new(point: TaskGatePoint, gate: Arc<tokio::sync::Mutex<()>>) -> Self {
        Self::counting(point, gate, Arc::default())
    }

    /// A gate that counts each task reaching it in `arrivals`.
    pub(crate) fn counting(
        point: TaskGatePoint,
        gate: Arc<tokio::sync::Mutex<()>>,
        arrivals: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        Self {
            point,
            gate,
            arrivals,
        }
    }

    fn shared(&self) -> Self {
        Self {
            point: self.point,
            gate: Arc::clone(&self.gate),
            arrivals: Arc::clone(&self.arrivals),
        }
    }

    async fn pass_at(&self, point: TaskGatePoint) {
        if self.point == point {
            self.arrivals.fetch_add(1, Ordering::SeqCst);
            drop(self.gate.lock().await);
        }
    }
}

#[cfg(test)]
impl<TKey> KeyedWorkerTasks<TKey> {
    pub(crate) fn hold(&mut self, gate: Option<TaskGate>) {
        self.gate = gate;
    }

    /// The first time the table is full, waits until every slot's report is queued
    /// before the loop marks that it waits for a slot.
    pub(crate) fn hold_full_table_until_reports_queued(&mut self) {
        self.hold_until_reports_queued = true;
    }

    /// How many reports are queued and not yet taken.
    pub(crate) fn reports_queued(&self) -> usize {
        self.finished.len()
    }

    async fn wait_for_every_report(&mut self) {
        if !std::mem::take(&mut self.hold_until_reports_queued) {
            return;
        }
        while self.finished.len() < self.capacity {
            tokio::task::yield_now().await;
        }
    }
}

#[cfg(test)]
#[path = "keyed_worker_tasks_test.rs"]
mod tests;
