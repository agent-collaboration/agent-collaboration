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

/// The modern object's single clock, shared with its id source.
#[derive(Clone, Default)]
pub(crate) struct WorkspaceClock {
    #[cfg(feature = "test-support")]
    held: std::sync::Arc<std::sync::Mutex<Option<SyncTime>>>,
}
impl WorkspaceClock {
    #[cfg(feature = "test-support")]
    pub(crate) fn hold(&self, milliseconds: i64) {
        *self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(SyncTime::from_unix_millis(milliseconds));
    }
}
impl Clock for WorkspaceClock {
    fn now(&self) -> SyncTime {
        #[cfg(feature = "test-support")]
        if let Some(time) = *self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            return time;
        }
        CloudflareClock.now()
    }
}
