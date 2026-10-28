//! Deterministic time input at the existing native registry clock boundary.
use runtime_primitives::{Clock, SyncTime};
use std::sync::atomic::{AtomicI64, Ordering};

pub struct ControlledClock(AtomicI64);
impl ControlledClock {
    pub fn at(time: SyncTime) -> Self {
        Self(AtomicI64::new(time.unix_millis()))
    }
    pub fn set(&self, time: SyncTime) {
        self.0.store(time.unix_millis(), Ordering::SeqCst);
    }
}
impl Clock for ControlledClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}
