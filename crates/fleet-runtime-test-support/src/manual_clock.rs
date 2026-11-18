use fleet_runtime_ports::{Clock, SyncTime};
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

/// Explicitly controlled time shared by the runtime, its store and its test.
#[derive(Clone)]
pub struct ManualClock(Arc<AtomicI64>);

impl ManualClock {
    pub fn starting_at(time: SyncTime) -> Self {
        Self(Arc::new(AtomicI64::new(time.unix_millis())))
    }

    pub fn set(&self, time: SyncTime) {
        self.0.store(time.unix_millis(), Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}
