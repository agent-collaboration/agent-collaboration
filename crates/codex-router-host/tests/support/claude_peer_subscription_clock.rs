//! Deterministic owner deadlines through the existing subscription clock seam.
use super::ProofResult;
use router_clock::RuntimeClock;

use router_clock::ManualClock;
use std::time::Duration;

// A clock step followed by wake_at(now) must not retain a later fixed Instant.
// This inherited SDK timer case remains clock C; it does not prove injectable waiting.
#[tokio::test]
async fn manual_advance_also_releases_an_already_armed_native_timer() {
    let proof: ProofResult = async {
        use runtime_primitives::{Clock, SyncTime, TokioWakeTimer, WakeScheduler};
        use std::{
            future::{Future, poll_fn},
            sync::Arc,
            task::Poll,
        };
        struct TimerClock(Arc<ManualClock>);
        impl Clock for TimerClock {
            fn now(&self) -> SyncTime {
                SyncTime::from_unix_millis(self.0.utc_now().timestamp_millis())
            }
        }
        let clock = Arc::new(ManualClock::new("2026-11-04T00:00:00Z".parse()?));
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
            .wake_at(SyncTime::from_unix_millis(
                clock.utc_now().timestamp_millis(),
            ))
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
