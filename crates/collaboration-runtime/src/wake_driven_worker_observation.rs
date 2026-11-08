//! What a [`crate::WakeDrivenWorkerLoop`] reports about itself. Production keeps
//! nothing; under test-support a test sees each store read at its read site, every pass,
//! the deadline armed before each sleep, whether the loop is asleep now, how many of the
//! worker's tasks have not reported yet, a barrier for "asleep" and a gate that holds the
//! loop after it decided to sleep and before it waits.
use runtime_primitives::SyncTime;

#[derive(Clone, Default)]
pub struct WorkerObservation {
    #[cfg(any(test, feature = "test-support"))]
    observed: std::sync::Arc<observed::ObservedLoop>,
}

impl WorkerObservation {
    /// Called by a worker pass at each store read it makes.
    pub fn record_read(&self, read: &'static str) {
        #[cfg(any(test, feature = "test-support"))]
        self.observed.record_read(read);
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = read;
    }

    pub(crate) fn pass_started(&self) {
        #[cfg(any(test, feature = "test-support"))]
        self.observed.pass_started();
    }

    /// The loop woke from its sleep and is about to run a pass.
    pub(crate) fn woke(&self) {
        #[cfg(any(test, feature = "test-support"))]
        self.observed.update(|snapshot| snapshot.asleep = false);
    }

    /// Reports how many of the worker's tasks are still running, until each reports.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn track_running_tasks(
        &self,
        running: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        if let Ok(mut counters) = self.observed.running.lock() {
            counters.push(running);
        }
    }

    /// The loop stopped waiting on a source that can never change again.
    pub(crate) fn source_closed(&self) {
        #[cfg(any(test, feature = "test-support"))]
        self.observed
            .update(|snapshot| snapshot.closed_sources += 1);
    }

    /// The loop is about to sleep with this deadline.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) async fn armed(&self, deadline: Option<SyncTime>) {
        self.observed.armed(deadline).await;
    }

    /// The loop is about to sleep with this deadline.
    #[cfg(not(any(test, feature = "test-support")))]
    pub(crate) fn armed(&self, deadline: Option<SyncTime>) -> std::future::Ready<()> {
        let _ = deadline;
        std::future::ready(())
    }

    /// A waiter calls this after the read that found nothing and before it waits, with the
    /// time its timer will fire: it records that time, counts the sleep and holds at the gate.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn read_found_nothing(&self, wakes_at: tokio::time::Instant) {
        self.observed
            .update(|snapshot| snapshot.waiter_wakes_at = Some(wakes_at));
        self.observed.slept(None).await;
    }

    /// A waiter calls this after the read that found nothing and before it waits.
    #[cfg(not(any(test, feature = "test-support")))]
    pub fn read_found_nothing(&self, wakes_at: tokio::time::Instant) -> std::future::Ready<()> {
        let _ = wakes_at;
        std::future::ready(())
    }
}

#[cfg(any(test, feature = "test-support"))]
pub use observed::{LoopSnapshot, SleepGate};

#[cfg(any(test, feature = "test-support"))]
mod observed {
    use super::WorkerObservation;
    use runtime_primitives::SyncTime;
    use std::sync::Mutex;
    use tokio::sync::{oneshot, watch};

    /// The loop's counters at one moment.
    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub struct LoopSnapshot {
        /// Each store read, in order, named by its read site.
        pub reads: Vec<&'static str>,
        pub passes: u64,
        pub sleeps: u64,
        /// Closed sources the loop removed from its wait set.
        pub closed_sources: u64,
        /// The deadline armed before the latest sleep; `None` means no timer.
        pub armed_deadline: Option<SyncTime>,
        /// The loop or waiter is waiting now, not running a pass or a read.
        pub asleep: bool,
        /// The worker's tasks that have not reported yet.
        pub tasks_running: usize,
        /// When a waiter's timer fires, as it reported before its latest sleep.
        pub waiter_wakes_at: Option<tokio::time::Instant>,
    }

    struct HeldSleep {
        reached: oneshot::Sender<()>,
        release: oneshot::Receiver<()>,
    }

    pub(super) struct ObservedLoop {
        snapshot: Mutex<LoopSnapshot>,
        sleeps: watch::Sender<u64>,
        held: Mutex<Option<HeldSleep>>,
        /// One counter per task set the worker runs.
        pub(super) running: Mutex<Vec<std::sync::Arc<std::sync::atomic::AtomicUsize>>>,
    }

    impl Default for ObservedLoop {
        fn default() -> Self {
            Self {
                snapshot: Mutex::default(),
                sleeps: watch::channel(0).0,
                held: Mutex::default(),
                running: Mutex::default(),
            }
        }
    }

    impl ObservedLoop {
        pub(super) fn update(&self, change: impl FnOnce(&mut LoopSnapshot)) {
            if let Ok(mut snapshot) = self.snapshot.lock() {
                change(&mut snapshot);
            }
        }

        pub(super) fn record_read(&self, read: &'static str) {
            self.update(|snapshot| {
                snapshot.reads.push(read);
                // A waiter reads after it wakes; a loop reads only during a pass.
                snapshot.asleep = false;
            });
        }

        pub(super) fn pass_started(&self) {
            self.update(|snapshot| {
                snapshot.passes += 1;
                snapshot.asleep = false;
            });
        }

        fn tasks_running(&self) -> usize {
            self.running.lock().map_or(0, |counters| {
                counters
                    .iter()
                    .map(|running| running.load(std::sync::atomic::Ordering::SeqCst))
                    .sum()
            })
        }

        pub(super) async fn armed(&self, deadline: Option<SyncTime>) {
            self.slept(Some(deadline)).await;
        }

        /// Holds at the gate if one is set, then counts the sleep. `armed` is the deadline
        /// a loop armed; a waiter passes `None` and leaves it unchanged.
        pub(super) async fn slept(&self, armed: Option<Option<SyncTime>>) {
            let held = self.held.lock().ok().and_then(|mut held| held.take());
            if let Some(held) = held {
                let _ = held.reached.send(());
                let _ = held.release.await;
            }
            let mut sleeps = 0;
            self.update(|snapshot| {
                snapshot.sleeps += 1;
                if let Some(deadline) = armed {
                    snapshot.armed_deadline = deadline;
                }
                snapshot.asleep = true;
                sleeps = snapshot.sleeps;
            });
            self.sleeps.send_replace(sleeps);
        }
    }

    /// Holds the loop after it decided to sleep and before it waits.
    pub struct SleepGate {
        reached: oneshot::Receiver<()>,
        release: oneshot::Sender<()>,
    }

    impl SleepGate {
        /// Resolves once the loop is held at the gate.
        pub async fn reached(&mut self) {
            let _ = (&mut self.reached).await;
        }

        /// Lets the held loop go to sleep.
        pub fn release(self) {
            let _ = self.release.send(());
        }
    }

    impl WorkerObservation {
        #[must_use]
        pub fn snapshot(&self) -> LoopSnapshot {
            let mut snapshot = self
                .observed
                .snapshot
                .lock()
                .map(|snapshot| snapshot.clone())
                .unwrap_or_default();
            snapshot.tasks_running = self.observed.tasks_running();
            snapshot
        }

        /// Resolves once the loop has gone to sleep at least `count` times; a loop that
        /// never started never resolves it.
        pub async fn asleep_at_least(&self, count: u64) {
            let mut sleeps = self.observed.sleeps.subscribe();
            let _ = sleeps.wait_for(|sleeps| *sleeps >= count).await;
        }

        /// Holds the loop the next time it is about to sleep.
        #[must_use]
        pub fn hold_before_next_sleep(&self) -> SleepGate {
            let (reached_sender, reached) = oneshot::channel();
            let (release, release_receiver) = oneshot::channel();
            if let Ok(mut held) = self.observed.held.lock() {
                *held = Some(HeldSleep {
                    reached: reached_sender,
                    release: release_receiver,
                });
            }
            SleepGate { reached, release }
        }
    }
}
