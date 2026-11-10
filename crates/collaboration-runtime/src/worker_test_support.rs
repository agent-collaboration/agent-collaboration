//! Worker observations and manual time; nextest supplies the hang bound.
use crate::WorkerObservation;
use router_clock::ManualClock;
use std::time::Duration;

pub(crate) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Waits, without moving the manual clock, until `condition` holds.
pub(crate) async fn wait_until(
    _what: &'static str,
    mut condition: impl FnMut() -> bool,
) -> TestResult {
    while !condition() {
        tokio::task::yield_now().await;
    }
    Ok(())
}

/// Lets in-flight work finish without moving the manual clock, then reports whether the
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
    while !settle(observation).await {
        tokio::task::yield_now().await;
    }
    Ok(())
}

/// As [`settled`], for a test that holds tasks on purpose.
pub(crate) async fn settled_holding(observation: &WorkerObservation) -> TestResult {
    while !settle_holding(observation).await {
        tokio::task::yield_now().await;
    }
    Ok(())
}

/// Advances the injected clock and yields to work made runnable by that advance.
pub(crate) async fn advance(clock: &ManualClock, by: Duration) {
    clock.advance(by).unwrap();
    for _ in 0..2_000 {
        tokio::task::yield_now().await;
    }
}
