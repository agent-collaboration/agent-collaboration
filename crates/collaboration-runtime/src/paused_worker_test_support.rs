//! Test-only helpers for driving a worker loop on Tokio's paused clock.
//!
//! Workers wait on SQLite's worker thread, so a paused runtime that looks idle would jump
//! to its next timer. `NoAutoAdvance` holds a blocking task, which stops Tokio's
//! auto-advance, so time moves only when a test advances it.
//!
//! Every wait is for a causal condition, bounded by real time so a wrong build fails
//! instead of hanging. The bound is generous: blocking SQLite work and real sockets take
//! as long as a loaded machine needs, and only a condition that never comes reaches it.
use crate::WorkerObservation;
use std::time::{Duration, Instant};

pub(crate) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// How long a test waits, in real time, for a condition before failing.
const COLLECTION_LIMIT: Duration = Duration::from_secs(30);

/// While this blocking task runs, Tokio's paused clock does not auto-advance.
pub(crate) struct NoAutoAdvance {
    release: Option<std::sync::mpsc::Sender<()>>,
}

impl NoAutoAdvance {
    pub(crate) fn hold() -> Self {
        let (release, released) = std::sync::mpsc::channel::<()>();
        drop(tokio::task::spawn_blocking(move || {
            let _ = released.recv();
        }));
        Self {
            release: Some(release),
        }
    }
}

impl Drop for NoAutoAdvance {
    fn drop(&mut self) {
        drop(self.release.take());
    }
}

/// The real-time bound of one wait. A wait whose condition needs async reads loops on
/// `next`, which yields to the work in flight and fails once the bound has passed.
pub(crate) struct CollectionLimit {
    what: &'static str,
    started: Instant,
}

impl CollectionLimit {
    pub(crate) fn start(what: &'static str) -> Self {
        Self {
            what,
            started: Instant::now(),
        }
    }

    pub(crate) async fn next(&self) -> TestResult {
        if self.started.elapsed() > COLLECTION_LIMIT {
            return Err(format!("{} within {COLLECTION_LIMIT:?}", self.what).into());
        }
        tokio::task::yield_now().await;
        Ok(())
    }
}

/// Waits, without moving the paused clock, until `condition` holds.
pub(crate) async fn wait_until(
    what: &'static str,
    mut condition: impl FnMut() -> bool,
) -> TestResult {
    let limit = CollectionLimit::start(what);
    while !condition() {
        limit.next().await?;
    }
    Ok(())
}

/// Lets in-flight work finish without moving the paused clock, then reports whether the
/// loop is asleep now, with every task reported and nothing new since.
pub(crate) async fn settle(observation: &WorkerObservation) -> bool {
    quiet(observation, TaskState::Reported).await
}

/// As [`settle`], for a test that holds tasks on purpose: those may still be running.
pub(crate) async fn settle_holding(observation: &WorkerObservation) -> bool {
    quiet(observation, TaskState::MayBeHeld).await
}

/// What a quiet loop requires of its tasks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Reported,
    MayBeHeld,
}

async fn quiet(observation: &WorkerObservation, tasks: TaskState) -> bool {
    let before = observation.snapshot();
    if !before.asleep || (tasks == TaskState::Reported && before.tasks_running > 0) {
        tokio::task::yield_now().await;
        return false;
    }
    for _ in 0..20 {
        for _ in 0..100 {
            tokio::task::yield_now().await;
        }
        if observation.snapshot() != before {
            return false;
        }
    }
    true
}

/// Waits, without moving the clock, until the loop is asleep with every task reported,
/// and stays so.
pub(crate) async fn settled(observation: &WorkerObservation) -> TestResult {
    let limit = CollectionLimit::start("the worker settled");
    while !settle(observation).await {
        limit.next().await?;
    }
    Ok(())
}

/// As [`settled`], for a test that holds tasks on purpose.
pub(crate) async fn settled_holding(observation: &WorkerObservation) -> TestResult {
    let limit = CollectionLimit::start("the worker settled with its held tasks");
    while !settle_holding(observation).await {
        limit.next().await?;
    }
    Ok(())
}

/// Moves both clocks with no work in flight.
pub(crate) async fn advance(by: Duration) {
    tokio::time::advance(by).await;
    for _ in 0..2_000 {
        tokio::task::yield_now().await;
    }
}
