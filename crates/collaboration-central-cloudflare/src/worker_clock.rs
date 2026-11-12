//! The Worker's one clock: the platform's time, read in one place.

use runtime_primitives::{Clock, SyncTime};

#[derive(Clone, Copy)]
pub(crate) struct CloudflareClock;

impl Clock for CloudflareClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(chrono::Utc::now().timestamp_millis())
    }
}
