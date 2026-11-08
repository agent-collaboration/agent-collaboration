//! The Tokio object's one alarm: lowered by units, set exactly by the recompute.
//!
//! Like Cloudflare's alarm it holds at most one wake time, and delivering it
//! consumes it. Business time comes from the object's clock, which conformance
//! may move independently of Tokio time, so each deadline also keeps the Tokio
//! instant it fires at; `clock_changed` recomputes that instant without lowering
//! or raising the stored time.

use collaboration_central_core::ObjectAlarm;
use runtime_primitives::{Clock, SyncTime, WakeFailure, WakeScheduler};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, Notify},
    time::Instant,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingAlarm {
    at: SyncTime,
    trigger: Instant,
}

pub struct ObjectAlarmTimer {
    clock: Arc<dyn Clock>,
    pending: Mutex<Option<PendingAlarm>>,
    changed: Notify,
    #[cfg(feature = "conformance")]
    settle: ClockSettle,
    #[cfg(feature = "conformance")]
    exact_faults: std::sync::Mutex<Vec<ExactWriteFault>>,
    /// Conformance only: the stored alarm is a seeded one the platform lost,
    /// so a clock move must not arm it. Any later alarm write clears this.
    #[cfg(feature = "conformance")]
    lost_delivery: std::sync::atomic::AtomicBool,
}

/// Conformance only: an armed failure of the exact write for one event kind.
#[cfg(feature = "conformance")]
struct ExactWriteFault {
    id: durable_steps::Uuid,
    on: collaboration_central_conformance::FaultedEvent,
    armed: u32,
    reached: u32,
    consumed: u32,
}

/// Conformance only: lets a clock move wait until the alarm loop has acted on it.
#[cfg(feature = "conformance")]
struct ClockSettle {
    /// Bumped by each `clock_changed`, after the new trigger is stored.
    epoch: std::sync::atomic::AtomicU64,
    /// The newest epoch the waiting loop evaluated and found nothing due for.
    settled: tokio::sync::watch::Sender<u64>,
}

impl ObjectAlarmTimer {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            pending: Mutex::new(None),
            changed: Notify::new(),
            #[cfg(feature = "conformance")]
            settle: ClockSettle {
                epoch: std::sync::atomic::AtomicU64::new(0),
                settled: tokio::sync::watch::channel(0).0,
            },
            #[cfg(feature = "conformance")]
            exact_faults: std::sync::Mutex::default(),
            #[cfg(feature = "conformance")]
            lost_delivery: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Wait for and consume the stored alarm. Only the object's alarm loop calls this.
    pub async fn wait(&self) -> SyncTime {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            // Register before reading, so a write between the read and the wait
            // still wakes this loop.
            notified.as_mut().enable();
            // Read the epoch before the alarm: an epoch this loop sees was
            // bumped after its trigger was stored, so the read below is current.
            #[cfg(feature = "conformance")]
            let epoch = self.settle.epoch.load(std::sync::atomic::Ordering::SeqCst);
            let pending = *self.pending.lock().await;
            #[cfg(feature = "conformance")]
            if pending.is_none_or(|expected| expected.trigger > Instant::now()) {
                self.settle.settled.send_replace(epoch);
            }
            match pending {
                None => notified.await,
                Some(expected) => {
                    tokio::select! {
                        () = &mut notified => {},
                        () = tokio::time::sleep_until(expected.trigger) => {
                            let mut current = self.pending.lock().await;
                            if *current == Some(expected) {
                                *current = None;
                                return expected.at;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Re-derive the firing instant after the object's clock moved. The stored
    /// time is unchanged, so this never manufactures or postpones an alarm.
    pub async fn clock_changed(&self) -> Result<(), WakeFailure> {
        let mut pending = self.pending.lock().await;
        #[cfg(feature = "conformance")]
        let lost = self.lost_delivery.load(std::sync::atomic::Ordering::SeqCst);
        #[cfg(not(feature = "conformance"))]
        let lost = false;
        if let Some(current) = *pending
            && !lost
        {
            *pending = Some(PendingAlarm {
                at: current.at,
                trigger: self.trigger_for(current.at)?,
            });
        }
        drop(pending);
        #[cfg(feature = "conformance")]
        self.settle
            .epoch
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.changed.notify_waiters();
        Ok(())
    }

    /// The epoch the next `settled_through` waits for: the latest clock change.
    #[cfg(feature = "conformance")]
    pub(crate) fn clock_epoch(&self) -> u64 {
        self.settle.epoch.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Resolves once the waiting loop has evaluated the stored alarm after the
    /// clock change `epoch` and found nothing due. An alarm the change made due
    /// is delivered, and its event completes, before that.
    #[cfg(feature = "conformance")]
    pub(crate) async fn settled_through(&self, epoch: u64) {
        let mut settled = self.settle.settled.subscribe();
        // The sender lives as long as the timer, so the wait can't fail.
        let _ = settled.wait_for(|settled| *settled >= epoch).await;
    }

    /// S5 seed: store `at` without arming delivery, as a platform that lost
    /// the alarm would. It stays until a recompute or repair replaces it.
    #[cfg(feature = "conformance")]
    pub(crate) async fn seed_stale(&self, at: SyncTime) -> Result<(), WakeFailure> {
        let never = Instant::now()
            .checked_add(Duration::from_secs(365 * 24 * 60 * 60))
            .ok_or(WakeFailure::InvalidDeadline { at })?;
        *self.pending.lock().await = Some(PendingAlarm { at, trigger: never });
        self.lost_delivery
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.changed.notify_waiters();
        Ok(())
    }

    #[cfg(feature = "conformance")]
    fn delivery_restored(&self) {
        self.lost_delivery
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn arm_exact_write_fault(
        &self,
        on: collaboration_central_conformance::FaultedEvent,
        count: u32,
    ) -> durable_steps::Uuid {
        let id = durable_steps::Uuid::now_v7();
        self.faults().push(ExactWriteFault {
            id,
            on,
            armed: count,
            reached: 0,
            consumed: 0,
        });
        id
    }

    /// (armed, reached, consumed) for one armed fault.
    #[cfg(feature = "conformance")]
    pub(crate) fn exact_write_fault(&self, id: durable_steps::Uuid) -> Option<(u32, u32, u32)> {
        self.faults()
            .iter()
            .find(|fault| fault.id == id)
            .map(|fault| (fault.armed, fault.reached, fault.consumed))
    }

    #[cfg(feature = "conformance")]
    fn faults(&self) -> std::sync::MutexGuard<'_, Vec<ExactWriteFault>> {
        self.exact_faults
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Whether an armed fault refuses this exact write, by the running event's kind.
    #[cfg(feature = "conformance")]
    fn exact_write_refused(&self) -> bool {
        use collaboration_central_conformance::FaultedEvent;
        use collaboration_central_core::event_trace::{EventClass, current_event};
        let on = match current_event() {
            Some(EventClass::Frame) => FaultedEvent::Frame,
            Some(EventClass::Call) => FaultedEvent::Call,
            Some(EventClass::Alarm) => FaultedEvent::Alarm,
            Some(EventClass::SocketOpened | EventClass::SocketClosed) | None => return false,
        };
        let mut faults = self.faults();
        let mut refused = false;
        for fault in faults.iter_mut().filter(|fault| fault.on == on) {
            fault.reached = fault.reached.saturating_add(1);
            if !refused && fault.consumed < fault.armed {
                fault.consumed += 1;
                refused = true;
            }
        }
        refused
    }

    fn trigger_for(&self, at: SyncTime) -> Result<Instant, WakeFailure> {
        let delay = u64::try_from(
            at.unix_millis()
                .saturating_sub(self.clock.now().unix_millis()),
        )
        .unwrap_or(0);
        Instant::now()
            .checked_add(Duration::from_millis(delay))
            .ok_or(WakeFailure::InvalidDeadline { at })
    }
}

impl WakeScheduler for ObjectAlarmTimer {
    async fn wake_at(&self, at: SyncTime) -> Result<(), WakeFailure> {
        self.lower(at).await?;
        #[cfg(feature = "conformance")]
        collaboration_central_core::event_trace::record_lower(at).await;
        Ok(())
    }
}

impl ObjectAlarmTimer {
    async fn lower(&self, at: SyncTime) -> Result<(), WakeFailure> {
        #[cfg(feature = "conformance")]
        self.delivery_restored();
        let mut pending = self.pending.lock().await;
        let trigger = self.trigger_for(at)?;
        let next = match *pending {
            Some(current) if current.at <= at => PendingAlarm {
                at: current.at,
                trigger: current.trigger.min(trigger),
            },
            Some(current) => PendingAlarm {
                at,
                trigger: current.trigger.min(trigger),
            },
            None => PendingAlarm { at, trigger },
        };
        if *pending == Some(next) {
            return Ok(());
        }
        *pending = Some(next);
        drop(pending);
        self.changed.notify_waiters();
        Ok(())
    }
}

impl ObjectAlarm for ObjectAlarmTimer {
    async fn set_exact(&self, at: Option<SyncTime>) -> Result<(), WakeFailure> {
        #[cfg(feature = "conformance")]
        if self.exact_write_refused() {
            return Err(WakeFailure::Unavailable {
                message: "conformance fault: exact alarm write".into(),
            });
        }
        #[cfg(feature = "conformance")]
        self.delivery_restored();
        let next = match at {
            Some(at) => Some(PendingAlarm {
                at,
                trigger: self.trigger_for(at)?,
            }),
            None => None,
        };
        let mut pending = self.pending.lock().await;
        if *pending == next {
            return Ok(());
        }
        *pending = next;
        drop(pending);
        self.changed.notify_waiters();
        Ok(())
    }

    async fn stored(&self) -> Result<Option<SyncTime>, WakeFailure> {
        Ok(self.pending.lock().await.map(|pending| pending.at))
    }
}

#[cfg(test)]
#[path = "object_alarm_timer_test.rs"]
mod tests;
