//! Redeliver failed native alarms even while their durable deadlines are unreadable.
use super::{RuntimeFace, SharedClock};
use runtime_primitives::{Clock, TokioWakeTimer, WakeScheduler};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAXIMUM_RETRY_DELAY: Duration = Duration::from_secs(30);

pub(super) async fn run(
    runtime: Arc<RuntimeFace>,
    wake: Arc<TokioWakeTimer<SharedClock>>,
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
    wake: &TokioWakeTimer<SharedClock>,
    clock: &SharedClock,
    retry_delay: &mut Duration,
) {
    if runtime.on_alarm().await.is_ok() {
        *retry_delay = INITIAL_RETRY_DELAY;
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
}
