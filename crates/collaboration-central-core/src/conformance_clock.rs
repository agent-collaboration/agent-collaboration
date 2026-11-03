//! A controlled Access timeline exists only in conformance builds.

use runtime_primitives::{Clock, SyncTime};
use std::sync::atomic::{AtomicI64, Ordering};

pub struct ConformanceAccessClock(AtomicI64);

impl ConformanceAccessClock {
    pub const INITIAL_MILLIS: i64 = 1_900_000_000_000;

    #[must_use]
    pub const fn new() -> Self {
        Self(AtomicI64::new(Self::INITIAL_MILLIS))
    }

    pub(crate) fn set(&self, now: SyncTime) {
        self.0.store(now.unix_millis(), Ordering::SeqCst);
    }
}

impl Default for ConformanceAccessClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ConformanceAccessClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}

pub(crate) fn observed_time(time: SyncTime) -> fleet_identity_protocol::UnixMillis {
    fleet_identity_protocol::UnixMillis(time.unix_millis())
}
