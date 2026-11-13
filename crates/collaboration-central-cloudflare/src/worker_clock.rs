//! The Worker's one clock: the platform's time, read in one place.

use runtime_primitives::{Clock, SyncTime};

#[derive(Clone, Copy)]
pub(crate) struct CloudflareClock;

impl Clock for CloudflareClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(chrono::Utc::now().timestamp_millis())
    }
}

/// One object's effective clock is shared by its timestamps and UUID source.
#[derive(Clone)]
pub(crate) struct ObjectClock {
    #[cfg(feature = "conformance")]
    faults: std::sync::Arc<collaboration_central_conformance::RuntimeFaults>,
}

impl ObjectClock {
    pub(crate) fn new(
        #[cfg(feature = "conformance")] faults: std::sync::Arc<
            collaboration_central_conformance::RuntimeFaults,
        >,
    ) -> Self {
        Self {
            #[cfg(feature = "conformance")]
            faults,
        }
    }
}

impl Clock for ObjectClock {
    fn now(&self) -> SyncTime {
        #[cfg(feature = "conformance")]
        if let Some(now) = self.faults.clock() {
            return SyncTime::from_unix_millis(now.0.timestamp_millis());
        }
        CloudflareClock.now()
    }
}
