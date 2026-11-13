//! A shared source clock, optionally controlled for one conformance object.

use runtime_primitives::{Clock, SyncTime};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct SharedClock {
    source: Arc<dyn Clock>,
    #[cfg(feature = "conformance")]
    faults: Option<Arc<collaboration_central_conformance::RuntimeFaults>>,
}

impl SharedClock {
    pub(crate) fn new(source: Arc<dyn Clock>) -> Self {
        Self {
            source,
            #[cfg(feature = "conformance")]
            faults: None,
        }
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn with_controls(
        mut self,
        faults: Arc<collaboration_central_conformance::RuntimeFaults>,
    ) -> Self {
        self.faults = Some(faults);
        self
    }
}

impl Clock for SharedClock {
    fn now(&self) -> SyncTime {
        #[cfg(feature = "conformance")]
        if let Some(now) = self.faults.as_ref().and_then(|faults| faults.clock()) {
            return SyncTime::from_unix_millis(now.0.timestamp_millis());
        }
        self.source.now()
    }
}
