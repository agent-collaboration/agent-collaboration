//! A moment on the injected monotonic clock, as the machines receive it.
//! Machines never read a clock: their callers pass the instant in.

use std::time::Duration;

/// Time elapsed since the injected clock's origin.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MonotonicInstant(Duration);

impl MonotonicInstant {
    /// The clock's origin.
    pub const ZERO: Self = Self(Duration::ZERO);

    /// The instant `elapsed` after the clock's origin.
    #[must_use]
    pub const fn from_elapsed(elapsed: Duration) -> Self {
        Self(elapsed)
    }

    /// Time since the clock's origin.
    #[must_use]
    pub const fn elapsed(self) -> Duration {
        self.0
    }

    /// The instant `delay` later.
    #[must_use]
    pub fn after(self, delay: Duration) -> Self {
        Self(self.0.saturating_add(delay))
    }

    /// Time from `earlier` to this instant; zero when `earlier` is later.
    #[must_use]
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}
