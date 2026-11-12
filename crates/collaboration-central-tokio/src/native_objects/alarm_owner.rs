//! Redeliver failed native alarms even while their durable deadlines are unreadable.
use super::{FleetRuntime, SharedClock};
use runtime_primitives::{Clock, TokioWakeTimer, WakeScheduler};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAXIMUM_RETRY_DELAY: Duration = Duration::from_secs(30);

pub(super) async fn run(
    runtime: Arc<FleetRuntime>,
    wake: Arc<TokioWakeTimer<SharedClock>>,
    clock: SharedClock,
    stop: CancellationToken,
) {
    let mut retry_delay = INITIAL_RETRY_DELAY;
    drive(&runtime, &wake, &clock, &mut retry_delay).await;
    loop {
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = wake.wait() => {
                drive(&runtime, &wake, &clock, &mut retry_delay).await;
            },
        }
    }
}

async fn drive(
    runtime: &FleetRuntime,
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
    // Use the timer's platform clock.
    if wake.wake_at(clock.now().after(*retry_delay)).await.is_err() {
        eprintln!("central alarm retry scheduling failed");
    }
    *retry_delay = retry_delay.saturating_mul(2).min(MAXIMUM_RETRY_DELAY);
}
