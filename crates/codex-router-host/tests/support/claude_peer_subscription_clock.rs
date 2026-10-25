//! Deterministic owner deadlines through the existing subscription clock seam.
use super::ProofResult;
use chrono::{DateTime, Utc};
use collaboration_service::SubscriptionClock;
use std::{sync::Mutex, time::Duration};
use tokio::time::Instant;

pub(super) struct ManualSubscriptionClock {
    wall: Mutex<DateTime<Utc>>,
    monotonic: Mutex<Instant>,
    advanced: tokio::sync::Notify,
}

impl ManualSubscriptionClock {
    pub(super) fn new(now: DateTime<Utc>) -> Self {
        Self {
            wall: Mutex::new(now),
            monotonic: Mutex::new(Instant::now()),
            advanced: tokio::sync::Notify::new(),
        }
    }

    pub(super) fn advance(&self, amount: Duration) -> ProofResult {
        let wall_amount = chrono::Duration::from_std(amount)?;
        let mut wall = self
            .wall
            .lock()
            .map_err(|_| std::io::Error::other("manual wall clock lock poisoned"))?;
        *wall = wall
            .checked_add_signed(wall_amount)
            .ok_or_else(|| std::io::Error::other("manual wall clock overflow"))?;
        let mut monotonic = self
            .monotonic
            .lock()
            .map_err(|_| std::io::Error::other("manual monotonic clock lock poisoned"))?;
        *monotonic = monotonic
            .checked_add(amount)
            .ok_or_else(|| std::io::Error::other("manual monotonic clock overflow"))?;
        drop(monotonic);
        drop(wall);
        self.advanced.notify_waiters();
        Ok(())
    }
}

impl SubscriptionClock for ManualSubscriptionClock {
    fn now(&self) -> DateTime<Utc> {
        *self
            .wall
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn monotonic_now(&self) -> Instant {
        *self
            .monotonic
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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

// A clock step followed by wake_at(now) must not retain a later fixed Instant.
// Decision 1007 keeps this consumer regression red until the library repair lands.
#[tokio::test]
async fn manual_advance_also_releases_an_already_armed_native_timer() {
    let proof: ProofResult = async {
        use runtime_primitives::{Clock, SyncTime, TokioWakeTimer, WakeScheduler};
        use std::{
            future::{Future, poll_fn},
            sync::Arc,
            task::Poll,
        };
        struct TimerClock(Arc<ManualSubscriptionClock>);
        impl Clock for TimerClock {
            fn now(&self) -> SyncTime {
                SyncTime::from_unix_millis(self.0.now().timestamp_millis())
            }
        }
        let clock = Arc::new(ManualSubscriptionClock::new(Utc::now()));
        let timer_clock = TimerClock(clock.clone());
        let due = timer_clock.now().after(Duration::from_secs(1));
        let timer = TokioWakeTimer::new(timer_clock);
        timer.wake_at(due).await?;
        let mut waiting = Box::pin(timer.wait());
        assert!(
            poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context)))
                .await
                .is_pending()
        );
        clock.advance(Duration::from_secs(1))?;
        timer
            .wake_at(SyncTime::from_unix_millis(clock.now().timestamp_millis()))
            .await?;
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(100), waiting)
                .await
                .expect("fixture time advanced but the already armed native Tokio timer did not"),
            due
        );
        Ok(())
    }
    .await;
    proof.expect("native timer must honor a shared-clock step");
}
