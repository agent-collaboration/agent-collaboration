//! Test-only helpers for driving a worker loop on Tokio's paused clock.
//!
//! Workers wait on SQLite's worker thread, so a paused runtime that looks idle would jump
//! to its next timer. `NoAutoAdvance` holds a blocking task, which stops Tokio's
//! auto-advance, so time moves only when a test advances it. Every wait is bounded, so a
//! wrong build fails instead of hanging.
use crate::WorkerObservation;
use std::time::Duration;

pub(crate) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// How many times a test waits on a condition before failing instead of hanging.
pub(crate) const WAIT_LIMIT: usize = 5_000;

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

/// Lets in-flight tasks finish without moving the paused clock, then reports whether
/// the loop is asleep with nothing new since.
pub(crate) async fn settle(observation: &WorkerObservation) -> bool {
    let before = observation.snapshot();
    for _ in 0..20 {
        for _ in 0..100 {
            tokio::task::yield_now().await;
        }
        if observation.snapshot() != before {
            return false;
        }
    }
    before.sleeps >= 1
}

/// Waits, without moving the clock, until the loop is asleep and stays so.
pub(crate) async fn settled(observation: &WorkerObservation) -> TestResult {
    for _ in 0..WAIT_LIMIT / 100 {
        if settle(observation).await {
            return Ok(());
        }
    }
    Err("the worker never settled".into())
}

/// Moves both clocks with no work in flight.
pub(crate) async fn advance(by: Duration) {
    tokio::time::advance(by).await;
    for _ in 0..2_000 {
        tokio::task::yield_now().await;
    }
}
