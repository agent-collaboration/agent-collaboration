//! One movable clock every Fleet object of a conformance server reads,
//! including a Fleet first created by a later provisioning.
use runtime_primitives::{Clock, SyncTime};
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

#[derive(Clone)]
pub(crate) struct FleetClock(Arc<AtomicI64>);

impl FleetClock {
    pub(crate) fn at(millis: i64) -> Self {
        Self(Arc::new(AtomicI64::new(millis)))
    }
    /// Move every Fleet's time; the caller then wakes live Fleets.
    pub(crate) fn set(&self, millis: i64) {
        self.0.store(millis, Ordering::SeqCst);
    }
    pub(crate) fn shared(&self) -> Arc<dyn Clock> {
        Arc::new(self.clone())
    }
}

impl Clock for FleetClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}
