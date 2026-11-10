//! The time capability Host creates once and passes to every time-consuming owner.
use chrono::{DateTime, Utc};
use runtime_primitives::{Clock, SyncTime};
use std::{future::Future, pin::Pin, time::Duration};

mod system_runtime_clock;
pub use system_runtime_clock::SystemRuntimeClock;

#[cfg(any(test, feature = "test-support"))]
mod manual_clock;
#[cfg(any(test, feature = "test-support"))]
pub use manual_clock::{ClockAdvanceError, ManualClock};

/// Elapsed time within one injected clock's lifetime; never a persisted timestamp.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct ClockInstant(Duration);

impl ClockInstant {
    pub const ZERO: Self = Self(Duration::ZERO);

    #[must_use]
    pub fn after(self, delay: Duration) -> Self {
        Self(self.0.saturating_add(delay))
    }

    #[must_use]
    pub fn checked_add(self, delay: Duration) -> Option<Self> {
        self.0.checked_add(delay).map(Self)
    }

    #[must_use]
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

impl std::ops::Add<Duration> for ClockInstant {
    type Output = Self;

    fn add(self, delay: Duration) -> Self {
        self.after(delay)
    }
}

impl std::ops::AddAssign<Duration> for ClockInstant {
    fn add_assign(&mut self, delay: Duration) {
        *self = self.after(delay);
    }
}

/// UTC and elapsed time have distinct meanings but one injected owner.
pub trait RuntimeClock: Clock {
    fn utc_now(&self) -> DateTime<Utc>;
    fn monotonic_now(&self) -> ClockInstant;
    fn sleep_until(&self, deadline: ClockInstant) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Test support can observe a wait on this object without exposing its concrete clock.
    #[cfg(any(test, feature = "test-support"))]
    fn has_not_due_check_now(&self, _deadline: ClockInstant) -> bool {
        false
    }

    /// Preserve a durable deadline's remaining delay when arming an elapsed timer.
    fn wall_deadline(&self, deadline: SyncTime) -> ClockInstant {
        let remaining_ms = deadline
            .unix_millis()
            .saturating_sub(self.now().unix_millis());
        self.monotonic_now().after(Duration::from_millis(
            u64::try_from(remaining_ms).unwrap_or(0),
        ))
    }

    fn sleep_for(&self, delay: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.sleep_until(self.monotonic_now().after(delay))
    }
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("the injected clock reached the wait deadline")]
pub struct WaitElapsed;

/// Bound an operation with the same clock that owns its caller's deadlines.
pub fn timeout<TFuture: Future>(
    clock: &dyn RuntimeClock,
    delay: Duration,
    operation: TFuture,
) -> impl Future<Output = Result<TFuture::Output, WaitElapsed>> {
    timeout_at(clock, clock.monotonic_now().after(delay), operation)
}

pub fn timeout_at<TFuture: Future>(
    clock: &dyn RuntimeClock,
    deadline: ClockInstant,
    operation: TFuture,
) -> impl Future<Output = Result<TFuture::Output, WaitElapsed>> {
    // Box before constructing the async wait, so large operation futures do not
    // inflate every caller's state machine. The deadline starts at creation.
    let operation = Box::pin(operation);
    async move {
        // Match Tokio's timeout: a future that consumes its cooperative budget must
        // still let the deadline be polled, while an already ready result wins.
        tokio::select! {
            biased;
            result = operation => Ok(result),
            () = tokio::task::unconstrained(clock.sleep_until(deadline)) => Err(WaitElapsed),
        }
    }
}
