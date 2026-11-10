//! The worker loop every converted Host worker runs: it sleeps until a commit, a
//! completion it waits on, its next due time or shutdown, and never on a fixed tick.
//!
//! One pass marks every wake source seen, drains the work batch after batch, and goes
//! again at once if a source changed while it worked. Then it reads the next due time
//! and sleeps on the sources plus one timer at that time; with no due time it holds no
//! timer. A pass whose read or unit fails retries from 1 s, doubling to 30 s, logs once
//! per run of failures, and the next pass that succeeds clears the retry.
use crate::wake_driven_worker_observation::WorkerObservation;
use futures_util::future::select_all;
use router_clock::RuntimeClock;
use runtime_primitives::SyncTime;
use sqlite_storage_maintenance::FailedPassBackoff;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

/// Something whose change makes work possible: a store's commit signal, a completion
/// counter or a readiness value.
pub trait WakeSource: Send {
    /// Marks the current value seen, so only a later change wakes the worker.
    fn mark_seen(&mut self);
    /// Whether the value changed since it was last marked seen.
    fn has_changed(&self) -> bool;
    /// Resolves at the next change. Resolves to `false` once the source can never change
    /// again, and the loop stops waiting on it.
    fn changed(&mut self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
}

impl<TValue: Send + Sync> WakeSource for watch::Receiver<TValue> {
    fn mark_seen(&mut self) {
        self.borrow_and_update();
    }

    fn has_changed(&self) -> bool {
        watch::Receiver::has_changed(self).unwrap_or(false)
    }

    fn changed(&mut self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move { watch::Receiver::changed(self).await.is_ok() })
    }
}

/// What one batch found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchOutcome {
    /// The batch was full or made progress that may have freed more work; run another.
    MoreWork,
    /// Nothing more is due now.
    Drained,
}

/// One worker's work, as the loop drives it. Every store read the pass makes reports
/// itself through `observation` at the read site.
pub trait WorkerPass: Send {
    type Error: std::fmt::Display + Send;

    /// Runs one batch of the work due at `now`.
    fn drain_batch(
        &mut self,
        now: SyncTime,
        observation: &WorkerObservation,
    ) -> impl Future<Output = Result<BatchOutcome, Self::Error>> + Send;

    /// The earliest time something not yet due becomes due, read with the same rule that
    /// selects that work. `None` means no timer.
    fn next_due(
        &mut self,
        now: SyncTime,
        observation: &WorkerObservation,
    ) -> impl Future<Output = Result<Option<SyncTime>, Self::Error>> + Send;
}

pub struct WakeDrivenWorkerLoop {
    worker: &'static str,
    clock: Arc<dyn RuntimeClock>,
    sources: Vec<Box<dyn WakeSource>>,
    observation: WorkerObservation,
}

impl WakeDrivenWorkerLoop {
    /// Subscribe each source with [`Self::wake_on`] before the loop's first read.
    pub fn new(worker: &'static str, clock: Arc<dyn RuntimeClock>) -> Self {
        Self {
            worker,
            clock,
            sources: Vec::new(),
            observation: WorkerObservation::default(),
        }
    }

    /// Reports to a handle its owner created first, so the owner can expose it before
    /// the loop exists.
    #[must_use]
    pub fn with_observation(mut self, observation: WorkerObservation) -> Self {
        self.observation = observation;
        self
    }

    #[must_use]
    pub fn wake_on(mut self, source: impl WakeSource + 'static) -> Self {
        self.sources.push(Box::new(source));
        self
    }

    /// The handle the pass reports its reads to; under test-support a test reads it.
    #[must_use]
    pub fn observation(&self) -> WorkerObservation {
        self.observation.clone()
    }

    /// Runs passes until `shutdown`. Startup is the first pass: it drains whatever an
    /// earlier process or `agent-router-update` committed.
    pub async fn run<TPass: WorkerPass>(mut self, pass: &mut TPass, shutdown: CancellationToken) {
        let mut failures = FailedPassBackoff::default();
        loop {
            // Shutdown is checked between units of work: a pass never starts after it, and
            // a batch already running finishes its transactions first.
            if shutdown.is_cancelled() {
                return;
            }
            for source in &mut self.sources {
                source.mark_seen();
            }
            self.observation.pass_started();
            tracing::debug!(target: "worker_loop", worker = self.worker, "pass started");
            let now = self.clock.now();
            let result = drain_and_read_next_due(pass, now, &self.observation, &shutdown).await;
            let deadline = match result {
                Ok(PassEnd::ShutDown) => return,
                Ok(PassEnd::Drained { next_due }) => {
                    failures.record_success();
                    next_due
                }
                Err(error) => {
                    let retry = failures.record_failure();
                    if retry.starts_run {
                        tracing::warn!(
                            worker = self.worker,
                            %error,
                            "worker pass failed; it retries with backoff"
                        );
                    }
                    Some(now.after(retry.delay))
                }
            };
            if self.sources.iter().any(|source| source.has_changed()) {
                continue;
            }
            tracing::debug!(
                target: "worker_loop",
                worker = self.worker,
                armed = ?deadline.map(SyncTime::unix_millis),
                "sleeping"
            );
            self.observation.armed(deadline).await;
            if !self.sleep_until_woken(deadline, &shutdown).await {
                return;
            }
            self.observation.woke();
        }
    }

    /// Sleeps until a source changes, the deadline passes or shutdown; `false` means
    /// shutdown.
    async fn sleep_until_woken(
        &mut self,
        deadline: Option<SyncTime>,
        shutdown: &CancellationToken,
    ) -> bool {
        let wait = deadline.map(|due| self.clock.wall_deadline(due));
        let clock = self.clock.clone();
        let mut timer = std::pin::pin!(async move {
            match wait {
                Some(deadline) => clock.sleep_until(deadline).await,
                None => std::future::pending::<()>().await,
            }
        });
        loop {
            let sources = &mut self.sources;
            let next_source_event = async move {
                if sources.is_empty() {
                    return std::future::pending::<SourceEvent>().await;
                }
                let (changed, index, _) =
                    select_all(sources.iter_mut().map(|source| source.changed())).await;
                if changed {
                    SourceEvent::Changed
                } else {
                    SourceEvent::Closed { index }
                }
            };
            let event = tokio::select! {
                biased;
                () = shutdown.cancelled() => return false,
                () = &mut timer => return true,
                event = next_source_event => event,
            };
            match event {
                SourceEvent::Changed => return true,
                // A closed source can never change again; stop waiting on it.
                SourceEvent::Closed { index } => {
                    self.sources.remove(index);
                    self.observation.source_closed();
                    // Hand control back once per source event, so no source can hold
                    // the runtime in a tight loop.
                    tokio::task::yield_now().await;
                }
            }
        }
    }
}

/// What the wake sources did while the loop slept.
enum SourceEvent {
    Changed,
    Closed { index: usize },
}

/// How one pass ended.
enum PassEnd {
    Drained {
        next_due: Option<SyncTime>,
    },
    /// Shutdown arrived; the batch that was running finished first.
    ShutDown,
}

async fn drain_and_read_next_due<TPass: WorkerPass>(
    pass: &mut TPass,
    now: SyncTime,
    observation: &WorkerObservation,
    shutdown: &CancellationToken,
) -> Result<PassEnd, TPass::Error> {
    while pass.drain_batch(now, observation).await? == BatchOutcome::MoreWork {
        if shutdown.is_cancelled() {
            return Ok(PassEnd::ShutDown);
        }
    }
    Ok(PassEnd::Drained {
        next_due: pass.next_due(now, observation).await?,
    })
}

#[cfg(test)]
#[path = "wake_driven_worker_loop_test.rs"]
mod tests;
