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
}

impl ObjectAlarmTimer {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            pending: Mutex::new(None),
            changed: Notify::new(),
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
            let pending = *self.pending.lock().await;
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
        let Some(current) = *pending else {
            return Ok(());
        };
        *pending = Some(PendingAlarm {
            at: current.at,
            trigger: self.trigger_for(current.at)?,
        });
        drop(pending);
        self.changed.notify_waiters();
        Ok(())
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
