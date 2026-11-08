//! Redeliver failed native alarms even while their durable deadlines are unreadable.
use super::ObjectSlot;
use super::object_slot::RuntimeFace;
use collaboration_central_core::{EventReply, ObjectEvent};
use runtime_primitives::{Clock, WakeScheduler};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAXIMUM_RETRY_DELAY: Duration = Duration::from_secs(30);

/// Consecutive failed deliveries of the current alarm, and the next backoff.
struct Redelivery {
    retries: u32,
    delay: Duration,
}

pub(super) async fn run(slot: Arc<ObjectSlot>, stop: CancellationToken) {
    let mut redelivery = Redelivery {
        retries: 0,
        delay: INITIAL_RETRY_DELAY,
    };
    #[cfg(feature = "conformance")]
    let mut duplicates = slot.io.seams.take_duplicate_requests();
    // Existing Copies are explicitly woken by startup discovery. Opening a
    // fresh Copy only for Health must create no migration or follow-up work.
    if !matches!(
        slot.current_runtime().as_deref(),
        Some(RuntimeFace::Copy(_))
    ) {
        drive(&slot, &mut redelivery, 0).await;
    }
    #[cfg(test)]
    slot.alarm_owner_generation.send_replace(1);
    loop {
        #[cfg(feature = "conformance")]
        let duplicate = async {
            match duplicates.as_mut() {
                Some(requests) => requests.recv().await,
                None => std::future::pending().await,
            }
        };
        #[cfg(not(feature = "conformance"))]
        let duplicate = std::future::pending::<Option<()>>();
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = slot.io.wake.wait() => {
                let delivered = redelivery.retries;
                drive(&slot, &mut redelivery, delivered).await;
            },
            Some(done) = duplicate => {
                // A duplicate of the alarm the loop last delivered.
                let delivered = redelivery.retries.saturating_add(1);
                drive(&slot, &mut redelivery, delivered).await;
                #[cfg(feature = "conformance")]
                let _ = done.send(());
                #[cfg(not(feature = "conformance"))]
                let () = done;
            },
        }
        #[cfg(test)]
        slot.alarm_owner_generation
            .send_modify(|value| *value = value.saturating_add(1));
    }
}

async fn drive(slot: &ObjectSlot, redelivery: &mut Redelivery, delivered: u32) {
    #[cfg(feature = "conformance")]
    if !slot.io.seams.admit_alarm(slot.io.now()) {
        return;
    }
    #[cfg(not(feature = "conformance"))]
    let _ = delivered;
    let reply = slot
        .on_event(
            ObjectEvent::Alarm,
            #[cfg(feature = "conformance")]
            collaboration_central_core::event_trace::EventContext {
                redelivery: Some(delivered),
            },
        )
        .await;
    if !matches!(reply, Some(EventReply::AlarmRetry)) {
        redelivery.retries = 0;
        redelivery.delay = INITIAL_RETRY_DELAY;
        return;
    }
    eprintln!("central alarm drive failed");
    // This is platform alarm redelivery, not a new journal attempt or deadline.
    // The timer keeps any earlier wake; the recovered handler consults the same
    // durable journal to decide which admitted operations are actually due.
    // The backoff is measured on the object's own clock, which drives its timer.
    if slot
        .io
        .wake
        .wake_at(slot.io.now().after(redelivery.delay))
        .await
        .is_err()
    {
        eprintln!("central alarm retry scheduling failed");
    }
    redelivery.retries = redelivery.retries.saturating_add(1);
    redelivery.delay = redelivery.delay.saturating_mul(2).min(MAXIMUM_RETRY_DELAY);
}

#[cfg(all(test, feature = "conformance"))]
#[path = "alarm_owner_test.rs"]
mod tests;
