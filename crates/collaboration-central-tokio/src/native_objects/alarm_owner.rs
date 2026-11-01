//! Redeliver failed native alarms even while their durable deadlines are unreadable.
use super::{NativeWake, RuntimeFace, SharedClock};
use runtime_primitives::{Clock, WakeScheduler};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAXIMUM_RETRY_DELAY: Duration = Duration::from_secs(30);

pub(super) async fn run(
    runtime: Arc<RuntimeFace>,
    wake: Arc<NativeWake>,
    clock: SharedClock,
    stop: CancellationToken,
    #[cfg(test)] ready: tokio::sync::watch::Sender<bool>,
) {
    let mut retry_delay = INITIAL_RETRY_DELAY;
    // Existing Copies are explicitly woken by startup discovery. Opening a
    // fresh Copy only for Health must create no migration or follow-up work.
    if !matches!(runtime.as_ref(), RuntimeFace::Copy(_)) {
        drive(&runtime, &wake, &clock, &mut retry_delay).await;
    }
    #[cfg(test)]
    ready.send_replace(true);
    loop {
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = wake.wait() => drive(&runtime, &wake, &clock, &mut retry_delay).await,
        }
    }
}

async fn drive(
    runtime: &RuntimeFace,
    wake: &NativeWake,
    clock: &SharedClock,
    retry_delay: &mut Duration,
) {
    let result = runtime.on_alarm().await;
    let failed = result.is_err();
    #[cfg(all(feature = "conformance", debug_assertions))]
    let after_handler_alarm_ms = wake.deadline().await;
    #[cfg(all(feature = "conformance", debug_assertions))]
    let failure = result.err().map(|error| error.to_string());
    if !failed {
        *retry_delay = INITIAL_RETRY_DELAY;
        #[cfg(all(feature = "conformance", debug_assertions))]
        wake.completed(None, after_handler_alarm_ms).await;
        return;
    }
    eprintln!("central alarm drive failed");
    // This is platform alarm redelivery, not a new journal attempt or deadline.
    // The timer keeps any earlier wake; the recovered handler consults the same
    // durable journal to decide which admitted operations are actually due.
    // Use the timer's platform clock, independent of conformance business time.
    if wake.wake_at(clock.now().after(*retry_delay)).await.is_err() {
        eprintln!("central alarm retry scheduling failed");
    }
    *retry_delay = retry_delay.saturating_mul(2).min(MAXIMUM_RETRY_DELAY);
    #[cfg(all(feature = "conformance", debug_assertions))]
    wake.completed(failure, after_handler_alarm_ms).await;
}
