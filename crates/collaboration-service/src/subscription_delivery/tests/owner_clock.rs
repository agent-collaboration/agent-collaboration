//! Durable UTC advances explicitly while owners can use runtime-driven retry timers.
use super::super::super::SubscriptionClock;
use chrono::{DateTime, Utc};
use std::sync::Mutex as StdMutex;
use tokio::time::Instant;

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
}
impl TestClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            wall: StdMutex::new(now),
            monotonic: StdMutex::new(Instant::now()),
            advanced: tokio::sync::Notify::new(),
        }
    }
    fn advance_wall_and_monotonic(&self, seconds: u64) {
        *self.wall.lock().unwrap() += chrono::Duration::seconds(i64::try_from(seconds).unwrap());
        *self.monotonic.lock().unwrap() += std::time::Duration::from_secs(seconds);
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
                if self.monotonic_now() >= deadline {
                    return;
                }
                advanced.await;
            }
        })
    }
}
