//! Debug-only clock control follows the same wall/monotonic seam as production readers.
use super::SubscriptionClock;
use chrono::{DateTime, Utc};
use std::sync::Mutex;
use tokio::sync::Notify;
use tokio::time::Instant;

pub struct DebugSubscriptionClock {
    time: Mutex<(DateTime<Utc>, Instant)>,
    advanced: Notify,
}
impl DebugSubscriptionClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            time: Mutex::new((now, Instant::now())),
            advanced: Notify::new(),
        }
    }
}
impl SubscriptionClock for DebugSubscriptionClock {
    fn now(&self) -> DateTime<Utc> {
        self.time
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0
    }
    fn monotonic_now(&self) -> Instant {
        self.time
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .1
    }
    fn debug_advance_to(&self, now: DateTime<Utc>) -> Result<(), &'static str> {
        let mut time = self.time.lock().map_err(|_| "debug clock lock poisoned")?;
        let elapsed = now
            .signed_duration_since(time.0)
            .to_std()
            .map_err(|_| "debug clock cannot move backwards")?;
        let monotonic = time.1.checked_add(elapsed).ok_or("debug clock overflow")?;
        *time = (now, monotonic);
        drop(time);
        self.advanced.notify_waiters();
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::poll_fn, task::Poll};
    #[tokio::test]
    async fn an_armed_deadline_is_released_by_a_clock_step_and_backwards_steps_are_refused() {
        let start = DateTime::from_timestamp_millis(1_000_000).unwrap();
        let clock = DebugSubscriptionClock::new(start);
        let deadline = clock.monotonic_now() + std::time::Duration::from_secs(600);
        let mut waiting = clock.sleep_until(deadline);
        assert!(
            poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context)))
                .await
                .is_pending()
        );
        clock
            .debug_advance_to(start + chrono::Duration::seconds(599))
            .unwrap();
        assert!(
            poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context)))
                .await
                .is_pending()
        );
        clock
            .debug_advance_to(start + chrono::Duration::seconds(600))
            .unwrap();
        assert!(
            poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context)))
                .await
                .is_ready()
        );
        assert!(clock.debug_advance_to(start).is_err());
        assert_eq!(clock.now(), start + chrono::Duration::seconds(600));
    }
}
