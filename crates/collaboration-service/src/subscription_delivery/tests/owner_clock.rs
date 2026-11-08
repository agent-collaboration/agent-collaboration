//! Durable UTC advances explicitly while owners can use runtime-driven retry timers.
use super::super::super::SubscriptionClock;
use chrono::{DateTime, Utc};
use std::sync::Mutex as StdMutex;
use tokio::time::Instant;

/// Finish real setup before paused time may auto-advance into the owner's timers.
pub async fn complete_real_setup<SetupOutput>(
    setup: impl std::future::Future<Output = SetupOutput>,
) -> SetupOutput {
    // Match the setup HTTP client's existing budget using a wall-clock bound.
    // The blocking guard follows the OAuth real-socket test pattern; explicit
    // advances remain available and no runtime clock is unpaused.
    let (release_clock, clock_release) = std::sync::mpsc::channel();
    let clock_guard = tokio::task::spawn_blocking(move || {
        clock_release.recv_timeout(std::time::Duration::from_secs(20))
    });
    let result = setup.await;
    release_clock.send(()).unwrap();
    clock_guard
        .await
        .unwrap()
        .expect("real fixture setup exceeded its existing 20-second budget");
    result
}

/// Real runtime timers may advance automatically; durable UTC stays under test control.
pub struct RuntimeTimerClock {
    wall_clock: std::sync::Arc<TestClock>,
}
impl RuntimeTimerClock {
    pub fn new(wall_clock: std::sync::Arc<TestClock>) -> Self {
        Self { wall_clock }
    }
}
impl SubscriptionClock for RuntimeTimerClock {
    fn now(&self) -> DateTime<Utc> {
        self.wall_clock.now()
    }
    fn monotonic_now(&self) -> Instant {
        Instant::now()
    }
}

pub struct TestClock {
    wall: StdMutex<DateTime<Utc>>,
    monotonic: StdMutex<Instant>,
    advanced: tokio::sync::Notify,
    /// Each (deadline, monotonic time of the check) a sleeper found not yet due.
    not_due_checks: StdMutex<Vec<(Instant, Instant)>>,
    not_due_checked: tokio::sync::Notify,
}
impl TestClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            wall: StdMutex::new(now),
            monotonic: StdMutex::new(Instant::now()),
            advanced: tokio::sync::Notify::new(),
            not_due_checks: StdMutex::new(Vec::new()),
            not_due_checked: tokio::sync::Notify::new(),
        }
    }
    fn advance_wall_and_monotonic(&self, seconds: u64) {
        self.advance_wall_and_monotonic_by(std::time::Duration::from_secs(seconds));
    }
    fn advance_wall_and_monotonic_by(&self, step: std::time::Duration) {
        *self.wall.lock().unwrap() += chrono::Duration::from_std(step).unwrap();
        *self.monotonic.lock().unwrap() += step;
    }

    /// Advances both clocks by any step, including part of a second, without Tokio time.
    pub fn advance_by_without_tokio_time(&self, step: std::time::Duration) {
        self.advance_wall_and_monotonic_by(step);
        self.advanced.notify_waiters();
    }

    /// Resolves once a sleeper waiting for `deadline` has looked at the clock as it reads
    /// now and found the deadline not yet due: proof it saw this time and kept waiting.
    pub async fn not_due_check_now(&self, deadline: Instant) {
        let now = self.monotonic_now();
        loop {
            let checked = self.not_due_checked.notified();
            tokio::pin!(checked);
            checked.as_mut().enable();
            if self
                .not_due_checks
                .lock()
                .unwrap()
                .contains(&(deadline, now))
            {
                return;
            }
            checked.await;
        }
    }

    pub async fn advance(&self, seconds: u64) {
        self.advance_wall_and_monotonic(seconds);
        tokio::time::advance(std::time::Duration::from_secs(seconds)).await;
        self.advanced.notify_waiters();
    }

    pub fn advance_without_tokio_time(&self, seconds: u64) {
        self.advance_wall_and_monotonic(seconds);
        self.advanced.notify_waiters();
    }
}
impl SubscriptionClock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        *self.wall.lock().unwrap()
    }
    fn monotonic_now(&self) -> Instant {
        *self.monotonic.lock().unwrap()
    }
    fn sleep_until(
        &self,
        deadline: Instant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            loop {
                let advanced = self.advanced.notified();
                tokio::pin!(advanced);
                advanced.as_mut().enable();
                let now = self.monotonic_now();
                if now >= deadline {
                    return;
                }
                self.not_due_checks.lock().unwrap().push((deadline, now));
                self.not_due_checked.notify_waiters();
                advanced.await;
            }
        })
    }
}
