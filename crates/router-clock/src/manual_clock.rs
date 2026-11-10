//! Shared test time; it never reads a native clock or arms a native timer.
use crate::{ClockInstant, RuntimeClock};
use chrono::{DateTime, Utc};
use runtime_primitives::{Clock, SyncTime};
use std::{future::Future, pin::Pin, time::Duration};
use tokio::sync::watch;

#[derive(Clone, Copy)]
struct ClockReading {
    utc: DateTime<Utc>,
    elapsed: ClockInstant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SleepCheck {
    deadline: ClockInstant,
    checked_at: ClockInstant,
}

#[derive(Debug, thiserror::Error)]
#[error("the manual clock advance exceeds its UTC or elapsed range")]
pub struct ClockAdvanceError;

pub struct ManualClock {
    reading: watch::Sender<ClockReading>,
    checks: watch::Sender<Vec<SleepCheck>>,
}

impl ManualClock {
    #[must_use]
    pub fn new(utc: DateTime<Utc>) -> Self {
        Self {
            reading: watch::channel(ClockReading {
                utc,
                elapsed: ClockInstant::ZERO,
            })
            .0,
            checks: watch::channel(Vec::new()).0,
        }
    }

    pub fn starting_at(utc: SyncTime) -> Result<Self, ClockAdvanceError> {
        DateTime::from_timestamp_millis(utc.unix_millis())
            .map(Self::new)
            .ok_or(ClockAdvanceError)
    }

    pub fn advance(&self, step: Duration) -> Result<(), ClockAdvanceError> {
        let delta = chrono::Duration::from_std(step).map_err(|_| ClockAdvanceError)?;
        let mut result = Ok(());
        self.reading.send_modify(|reading| {
            match (
                reading.utc.checked_add_signed(delta),
                reading.elapsed.checked_add(step),
            ) {
                (Some(utc), Some(elapsed)) => *reading = ClockReading { utc, elapsed },
                _ => result = Err(ClockAdvanceError),
            }
        });
        result
    }

    pub fn advance_to(&self, deadline: SyncTime) -> Result<(), ClockAdvanceError> {
        let step_ms = deadline
            .unix_millis()
            .checked_sub(self.now().unix_millis())
            .and_then(|step| u64::try_from(step).ok())
            .ok_or(ClockAdvanceError)?;
        self.advance(Duration::from_millis(step_ms))
    }

    /// A causal observation: this deadline was checked at the current manual time
    /// and was not due. A direct native wait cannot produce this observation.
    pub async fn not_due_check_now(&self, deadline: ClockInstant) {
        let expected = SleepCheck {
            deadline,
            checked_at: self.monotonic_now(),
        };
        let mut checks = self.checks.subscribe();
        let _ = checks.wait_for(|checks| checks.contains(&expected)).await;
    }

    #[must_use]
    pub fn has_not_due_check_now(&self, deadline: ClockInstant) -> bool {
        self.checks.borrow().contains(&SleepCheck {
            deadline,
            checked_at: self.monotonic_now(),
        })
    }

    fn record_sleep_check(&self, check: SleepCheck) {
        self.checks.send_if_modified(|checks| {
            if checks.contains(&check) {
                false
            } else {
                checks.push(check);
                true
            }
        });
    }
}

impl Clock for ManualClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.utc_now().timestamp_millis())
    }
}

impl RuntimeClock for ManualClock {
    fn has_not_due_check_now(&self, deadline: ClockInstant) -> bool {
        ManualClock::has_not_due_check_now(self, deadline)
    }

    fn utc_now(&self) -> DateTime<Utc> {
        self.reading.borrow().utc
    }

    fn monotonic_now(&self) -> ClockInstant {
        self.reading.borrow().elapsed
    }

    fn wall_deadline(&self, deadline: SyncTime) -> ClockInstant {
        // One snapshot prevents an advance between the UTC and elapsed reads
        // from adding that advance to the remaining delay.
        let reading = *self.reading.borrow();
        let remaining_ms = deadline
            .unix_millis()
            .saturating_sub(reading.utc.timestamp_millis());
        reading.elapsed.after(Duration::from_millis(
            u64::try_from(remaining_ms).unwrap_or(0),
        ))
    }

    fn sleep_until(&self, deadline: ClockInstant) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let mut changes = self.reading.subscribe();
            loop {
                let now = changes.borrow_and_update().elapsed;
                if now >= deadline {
                    return;
                }
                self.record_sleep_check(SleepCheck {
                    deadline,
                    checked_at: now,
                });
                if changes.changed().await.is_err() {
                    return;
                }
            }
        })
    }
}

#[cfg(test)]
#[path = "manual_clock_test.rs"]
mod tests;
